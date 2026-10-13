//! App desktop AIO Backup: lớp mỏng bọc backup-core.
//! - Command cho UI (config, ổ đĩa, job, mật khẩu FTP, chạy/huỷ, lịch sử)
//! - Thread watcher: cắm USB đích -> chạy job auto_run; job có lịch -> chạy định kỳ
//! - Tray icon: đóng cửa sổ chỉ ẩn xuống tray, app vẫn chạy nền

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use backup_core::config::{default_config_path, Config, Destination, FtpTarget, Job};
use backup_core::drives::{list_drives, DriveInfo};
use backup_core::engine::{Progress, Report};
use backup_core::history::{self, HistoryEntry};
use backup_core::job::{locate_target, open_storage, run_job_on};
use backup_core::marker::{init_target, TargetMarker};
use backup_core::restore::{self, RestoreEntry, RestoreOptions, RestoreReport, SnapshotInfo};
use backup_core::schedule::{Scheduler, Trigger};
use backup_core::secrets;
use backup_core::storage::ftp::test_connection;
use serde::Serialize;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_notification::NotificationExt;
use uuid::Uuid;

struct AppState {
    cfg_path: PathBuf,
    config: Mutex<Config>,
    /// job_id -> cờ huỷ của lượt đang chạy
    running: Mutex<HashMap<Uuid, Arc<AtomicBool>>>,
    scheduler: Mutex<Scheduler>,
    /// Cờ huỷ của lượt khôi phục đang chạy (mỗi lúc chỉ một lượt).
    restore_cancel: Mutex<Option<Arc<AtomicBool>>>,
}

type CmdResult<T> = Result<T, String>;

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct ProgressEvent {
    job_id: Uuid,
    progress: Progress,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct FinishedEvent {
    job_id: Uuid,
    job_name: String,
    report: Option<Report>,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleInfo {
    job_id: Uuid,
    last_run: Option<chrono::DateTime<chrono::Utc>>,
    next_run: Option<chrono::DateTime<chrono::Utc>>,
    /// USB: ổ đích đang cắm. FTP: luôn true (không biết trước khi kết nối).
    available: bool,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct RestoreProgress {
    done: usize,
    total: usize,
    current: String,
}

#[derive(Serialize)]
struct SaveResult {
    config: Config,
    warning: Option<String>,
}

/// Bản config gửi lên UI: không bao giờ kèm mật khẩu.
fn redacted(cfg: &Config) -> Config {
    let mut c = cfg.clone();
    for j in &mut c.jobs {
        if let Destination::Ftp(f) = &mut j.destination {
            f.password = None;
        }
    }
    c
}

// ---------- Commands (async để không chặn main thread) ----------

#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> CmdResult<Config> {
    Ok(redacted(&state.config.lock().unwrap()))
}

#[tauri::command]
async fn get_drives() -> CmdResult<Vec<DriveInfo>> {
    Ok(list_drives())
}

#[tauri::command]
async fn mark_target(path: String, label: String, force: bool) -> CmdResult<TargetMarker> {
    init_target(&PathBuf::from(path), &label, force).map_err(|e| e.to_string())
}

/// Thêm mới hoặc cập nhật job (theo id). `password` chỉ dùng cho FTP;
/// bỏ trống khi sửa job = giữ mật khẩu cũ.
#[tauri::command]
async fn save_job(state: State<'_, AppState>, mut job: Job, password: Option<String>) -> CmdResult<SaveResult> {
    if job.name.trim().is_empty() {
        return Err("Tên job không được trống".into());
    }
    if !job.source.is_dir() {
        return Err(format!("Thư mục nguồn không tồn tại: {}", job.source.display()));
    }
    if job.schedule.as_ref().is_some_and(|s| !s.is_valid()) {
        return Err("Lịch chạy không hợp lệ: chọn ít nhất một thứ trong tuần, ngày trong tháng từ 1 đến 31".into());
    }

    let mut cfg = state.config.lock().unwrap();
    if cfg
        .jobs
        .iter()
        .any(|j| j.id != job.id && j.name.eq_ignore_ascii_case(&job.name))
    {
        return Err(format!("Đã có job tên '{}'", job.name));
    }

    let mut warning = None;
    if let Destination::Ftp(ftp) = &mut job.destination {
        if ftp.host.trim().is_empty() || ftp.username.trim().is_empty() {
            return Err("Nhập host và tên đăng nhập FTP".into());
        }
        let password = password.filter(|p| !p.is_empty());
        // UI không gửi lại mật khẩu đã lưu trong config -> giữ bản cũ
        if password.is_none() {
            ftp.password = cfg
                .jobs
                .iter()
                .find(|j| j.id == job.id)
                .and_then(|j| j.destination.ftp())
                .and_then(|f| f.password.clone());
        }
        match password {
            Some(p) => warning = secrets::set_job_password(&mut job, &p),
            None if secrets::load_password(&job).is_none() => {
                return Err("Nhập mật khẩu FTP".into());
            }
            None => {}
        }
    }

    let old_schedule = cfg.jobs.iter().find(|j| j.id == job.id).and_then(|j| j.schedule.as_ref());
    if job.schedule.is_some() && job.schedule.as_ref() != old_schedule {
        state.scheduler.lock().unwrap().reschedule(job.id, chrono::Utc::now());
    }

    match cfg.jobs.iter_mut().find(|j| j.id == job.id) {
        Some(existing) => *existing = job,
        None => cfg.jobs.push(job),
    }
    cfg.save(&state.cfg_path).map_err(|e| e.to_string())?;
    Ok(SaveResult {
        config: redacted(&cfg),
        warning,
    })
}

#[tauri::command]
async fn delete_job(state: State<'_, AppState>, id: Uuid) -> CmdResult<Config> {
    let mut cfg = state.config.lock().unwrap();
    cfg.jobs.retain(|j| j.id != id);
    cfg.save(&state.cfg_path).map_err(|e| e.to_string())?;
    secrets::delete_password(id);
    Ok(redacted(&cfg))
}

/// Kiểm tra kết nối FTP từ form. Mật khẩu trống = dùng mật khẩu đã lưu của job.
#[tauri::command]
async fn test_ftp(
    state: State<'_, AppState>,
    ftp: FtpTarget,
    password: Option<String>,
    job_id: Option<Uuid>,
) -> CmdResult<String> {
    let password = match password.filter(|p| !p.is_empty()) {
        Some(p) => p,
        None => job_id
            .and_then(|id| state.config.lock().unwrap().jobs.iter().find(|j| j.id == id).cloned())
            .and_then(|j| secrets::load_password(&j))
            .ok_or("Nhập mật khẩu để kiểm tra")?,
    };
    tauri::async_runtime::spawn_blocking(move || test_connection(&ftp, &password))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn start_job(app: AppHandle, state: State<'_, AppState>, id: Uuid, dry_run: bool) -> CmdResult<()> {
    let job = state
        .config
        .lock()
        .unwrap()
        .jobs
        .iter()
        .find(|j| j.id == id)
        .cloned()
        .ok_or("Job không tồn tại")?;
    if !dry_run {
        state.scheduler.lock().unwrap().record_run(id, chrono::Utc::now());
    }
    spawn_job(&app, job, dry_run, false)
}

#[tauri::command]
async fn cancel_job(state: State<'_, AppState>, id: Uuid) -> CmdResult<()> {
    if let Some(flag) = state.running.lock().unwrap().get(&id) {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
async fn running_jobs(state: State<'_, AppState>) -> CmdResult<Vec<Uuid>> {
    Ok(state.running.lock().unwrap().keys().copied().collect())
}

#[tauri::command]
async fn get_history(limit: usize) -> CmdResult<Vec<HistoryEntry>> {
    Ok(history::recent(&history::default_history_path(), limit))
}

/// Lần chạy gần nhất / kế tiếp của từng job, cho trang Tổng quan.
#[tauri::command]
async fn get_schedule(state: State<'_, AppState>) -> CmdResult<Vec<ScheduleInfo>> {
    let jobs = state.config.lock().unwrap().jobs.clone();
    let present: HashSet<Uuid> = list_drives()
        .into_iter()
        .filter_map(|d| d.marker.map(|m| m.id))
        .collect();
    let now = chrono::Utc::now();
    let sched = state.scheduler.lock().unwrap();
    Ok(jobs
        .iter()
        .map(|j| {
            let available = match &j.destination {
                Destination::Usb { target_id } => present.contains(target_id),
                Destination::Ftp(_) => true,
            };
            ScheduleInfo {
                job_id: j.id,
                last_run: sched.last_run(j.id),
                next_run: if available { sched.next_run(j, now) } else { None },
                available,
            }
        })
        .collect())
}

#[tauri::command]
async fn save_settings(
    state: State<'_, AppState>,
    poll_interval_secs: u64,
    default_excludes: Vec<String>,
    default_keep_days: u32,
) -> CmdResult<Config> {
    let mut cfg = state.config.lock().unwrap();
    cfg.poll_interval_secs = poll_interval_secs.clamp(2, 3600);
    cfg.default_excludes = default_excludes
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    cfg.default_keep_days = default_keep_days;
    cfg.save(&state.cfg_path).map_err(|e| e.to_string())?;
    Ok(redacted(&cfg))
}

fn find_job(state: &AppState, id: Uuid) -> CmdResult<Job> {
    state
        .config
        .lock()
        .unwrap()
        .jobs
        .iter()
        .find(|j| j.id == id)
        .cloned()
        .ok_or_else(|| "Job không tồn tại".to_string())
}

#[tauri::command]
async fn list_snapshots(state: State<'_, AppState>, job_id: Uuid) -> CmdResult<Vec<SnapshotInfo>> {
    let job = find_job(&state, job_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut st = open_storage(&job)?;
        restore::list_snapshots(st.as_mut())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_restore_entries(
    state: State<'_, AppState>,
    job_id: Uuid,
    snapshot: Option<String>,
) -> CmdResult<Vec<RestoreEntry>> {
    let job = find_job(&state, job_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut st = open_storage(&job)?;
        restore::list_entries(st.as_mut(), snapshot.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn restore_files(
    app: AppHandle,
    state: State<'_, AppState>,
    job_id: Uuid,
    snapshot: Option<String>,
    paths: Vec<String>,
    target: String,
    overwrite: bool,
) -> CmdResult<RestoreReport> {
    let job = find_job(&state, job_id)?;
    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut slot = state.restore_cancel.lock().unwrap();
        if slot.is_some() {
            return Err("Đang có một lượt khôi phục khác".into());
        }
        *slot = Some(cancel.clone());
    }
    let app2 = app.clone();
    let res = tauri::async_runtime::spawn_blocking(move || {
        let mut st = open_storage(&job)?;
        let opts = RestoreOptions {
            target: PathBuf::from(target),
            overwrite,
        };
        restore::restore(
            st.as_mut(),
            snapshot.as_deref(),
            &paths,
            &opts,
            &mut |done, total, current| {
                let _ = app2.emit(
                    "restore-progress",
                    RestoreProgress {
                        done,
                        total,
                        current: current.to_string(),
                    },
                );
            },
            &cancel,
        )
    })
    .await;
    *app.state::<AppState>().restore_cancel.lock().unwrap() = None;
    res.map_err(|e| e.to_string())?.map_err(|e| e.to_string())
}

#[tauri::command]
async fn cancel_restore(state: State<'_, AppState>) -> CmdResult<()> {
    if let Some(c) = state.restore_cancel.lock().unwrap().as_ref() {
        c.store(true, Ordering::Relaxed);
    }
    Ok(())
}

// ---------- Chạy job trên thread riêng ----------

fn spawn_job(app: &AppHandle, job: Job, dry_run: bool, notify: bool) -> CmdResult<()> {
    let state = app.state::<AppState>();
    // USB chưa cắm thì báo ngay, khỏi tạo thread
    if matches!(job.destination, Destination::Usb { .. }) {
        locate_target(&job).map_err(|e| e.to_string())?;
    }

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let mut running = state.running.lock().unwrap();
        if running.contains_key(&job.id) {
            return Err(format!("Job '{}' đang chạy", job.name));
        }
        running.insert(job.id, cancel.clone());
    }
    let _ = app.emit("job-started", job.id);

    let app = app.clone();
    std::thread::spawn(move || {
        let job_id = job.id;
        let started_at = chrono::Utc::now();
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        // Kết nối FTP có thể mất vài giây -> làm trong thread
        let result = open_storage(&job).and_then(|mut storage| {
            run_job_on(
                &job,
                storage.as_mut(),
                dry_run,
                &mut |p| {
                    // Giới hạn ~10 event/giây cho tiến độ từng file để UI không bị dội
                    let is_file = matches!(p, Progress::File { .. });
                    if !is_file || last_emit.elapsed() >= Duration::from_millis(100) {
                        last_emit = Instant::now();
                        let _ = app.emit("job-progress", ProgressEvent { job_id, progress: p });
                    }
                },
                &cancel,
            )
        });

        app.state::<AppState>().running.lock().unwrap().remove(&job_id);

        let (report, error) = match result {
            Ok(r) => {
                let _ = history::append(&history::default_history_path(), job.id, &job.name, &r);
                (Some(r), None)
            }
            Err(e) => {
                let msg = e.to_string();
                let _ = history::append_error(
                    &history::default_history_path(),
                    job.id,
                    &job.name,
                    started_at,
                    dry_run,
                    &msg,
                );
                (None, Some(msg))
            }
        };

        // Lượt tự động lỗi cũng báo, để người dùng biết NAS/USB có vấn đề
        if notify {
            let body = match (&report, &error) {
                (Some(r), _) if r.ok() => format!(
                    "Xong: +{} mới, ~{} cập nhật, -{} xoá",
                    r.created, r.updated, r.deleted
                ),
                (Some(r), _) if r.target_lost => "Mất kết nối tới đích giữa chừng".to_string(),
                (Some(r), _) => format!("Kết thúc với {} lỗi", r.failed.len()),
                (None, Some(e)) => format!("Lỗi: {e}"),
                _ => String::new(),
            };
            let _ = app
                .notification()
                .builder()
                .title(format!("AIO Backup: {}", job.name))
                .body(body)
                .show();
        }

        let _ = app.emit(
            "job-finished",
            FinishedEvent {
                job_id,
                job_name: job.name.clone(),
                report,
                error,
            },
        );
    });
    Ok(())
}

// ---------- Theo dõi USB + lịch định kỳ ----------

fn watcher(app: AppHandle) {
    let mut last_mounts: Vec<PathBuf> = vec![];
    let mut last_present: HashSet<Uuid> = HashSet::new();
    loop {
        let drives = list_drives();
        let mounts: Vec<PathBuf> = drives.iter().map(|d| d.mount_point.clone()).collect();
        let present: HashSet<Uuid> = drives
            .iter()
            .filter_map(|d| d.marker.as_ref().map(|m| m.id))
            .collect();
        if mounts != last_mounts || present != last_present {
            let _ = app.emit("drives-changed", &drives);
        }

        let state = app.state::<AppState>();
        let (jobs, poll) = {
            let cfg = state.config.lock().unwrap();
            (cfg.jobs.clone(), cfg.poll_interval_secs.max(1))
        };
        let due = state
            .scheduler
            .lock()
            .unwrap()
            .tick(&jobs, &present, chrono::Utc::now());
        for d in due {
            let why = match d.trigger {
                Trigger::Plugged => "ổ đích vừa cắm",
                Trigger::Interval => "tới lịch",
            };
            tracing::info!("chạy {} ({why})", d.job.name);
            if let Err(e) = spawn_job(&app, d.job.clone(), false, true) {
                tracing::warn!("auto-run {} lỗi: {e}", d.job.name);
            }
        }

        last_mounts = mounts;
        last_present = present;
        std::thread::sleep(Duration::from_secs(poll));
    }
}

// ---------- Cửa sổ & tray ----------

fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,suppaftp=off".into()),
        )
        .init();

    let cfg_path = default_config_path();
    let config = Config::load(&cfg_path).unwrap_or_else(|e| {
        tracing::error!("Không đọc được config, dùng mặc định: {e}");
        Config::default()
    });
    let scheduler = Scheduler::new(history::last_runs(&history::default_history_path()));

    tauri::Builder::default()
        // Phải đăng ký đầu tiên: mở app lần 2 thì chỉ hiện lại cửa sổ cũ
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| show_main(app)))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--minimized"]),
        ))
        .manage(AppState {
            cfg_path,
            config: Mutex::new(config),
            running: Mutex::new(HashMap::new()),
            scheduler: Mutex::new(scheduler),
            restore_cancel: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            get_drives,
            mark_target,
            save_job,
            delete_job,
            test_ftp,
            start_job,
            cancel_job,
            running_jobs,
            get_history,
            get_schedule,
            save_settings,
            list_snapshots,
            list_restore_entries,
            restore_files,
            cancel_restore,
        ])
        .setup(|app| {
            let show = MenuItem::with_id(app, "show", "Mở AIO Backup", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Thoát", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("AIO Backup")
                .menu(&menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "quit" => app.exit(0),
                    _ => {}
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            // Khởi động cùng hệ thống -> chạy ẩn dưới tray
            if std::env::args().any(|a| a == "--minimized") {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }

            let handle = app.handle().clone();
            std::thread::spawn(move || watcher(handle));
            Ok(())
        })
        .on_window_event(|window, event| {
            // Đóng cửa sổ = ẩn xuống tray, watcher vẫn chạy
            if let WindowEvent::CloseRequested { api, .. } = event {
                let _ = window.hide();
                api.prevent_close();
            }
        })
        .run(tauri::generate_context!())
        .expect("Không khởi động được ứng dụng");
}
