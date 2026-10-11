//! App desktop: lớp mỏng bọc backup-core.
//! - Command cho UI (config, ổ đĩa, chạy/huỷ job, lịch sử)
//! - Thread watcher: poll ổ đĩa, cắm USB có marker -> tự chạy job auto_run
//! - Tray icon: đóng cửa sổ chỉ ẩn xuống tray, app vẫn chạy nền

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use backup_core::config::{default_config_path, Config, Job};
use backup_core::drives::{list_drives, DriveInfo};
use backup_core::engine::{Progress, Report};
use backup_core::history::{self, HistoryEntry};
use backup_core::job::{locate_target, run_job_at};
use backup_core::marker::{init_target, TargetMarker};
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

// ---------- Commands (async để không chặn main thread) ----------

#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> CmdResult<Config> {
    Ok(state.config.lock().unwrap().clone())
}

#[tauri::command]
async fn get_drives() -> CmdResult<Vec<DriveInfo>> {
    Ok(list_drives())
}

#[tauri::command]
async fn mark_target(path: String, label: String, force: bool) -> CmdResult<TargetMarker> {
    init_target(&PathBuf::from(path), &label, force).map_err(|e| e.to_string())
}

/// Thêm mới hoặc cập nhật job (theo id).
#[tauri::command]
async fn save_job(state: State<'_, AppState>, job: Job) -> CmdResult<Config> {
    if job.name.trim().is_empty() {
        return Err("Tên job không được trống".into());
    }
    if !job.source.is_dir() {
        return Err(format!("Thư mục nguồn không tồn tại: {}", job.source.display()));
    }
    let mut cfg = state.config.lock().unwrap();
    if cfg
        .jobs
        .iter()
        .any(|j| j.id != job.id && j.name.eq_ignore_ascii_case(&job.name))
    {
        return Err(format!("Đã có job tên '{}'", job.name));
    }
    match cfg.jobs.iter_mut().find(|j| j.id == job.id) {
        Some(existing) => *existing = job,
        None => cfg.jobs.push(job),
    }
    cfg.save(&state.cfg_path).map_err(|e| e.to_string())?;
    Ok(cfg.clone())
}

#[tauri::command]
async fn delete_job(state: State<'_, AppState>, id: Uuid) -> CmdResult<Config> {
    let mut cfg = state.config.lock().unwrap();
    cfg.jobs.retain(|j| j.id != id);
    cfg.save(&state.cfg_path).map_err(|e| e.to_string())?;
    Ok(cfg.clone())
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

// ---------- Chạy job trên thread riêng ----------

fn spawn_job(app: &AppHandle, job: Job, dry_run: bool, notify: bool) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let root = locate_target(&job).map_err(|e| e.to_string())?;

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
        let mut last_emit = Instant::now() - Duration::from_secs(1);
        let result = run_job_at(
            &job,
            &root,
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
        );

        app.state::<AppState>().running.lock().unwrap().remove(&job_id);

        let (report, error) = match result {
            Ok(r) => {
                let _ = history::append(&history::default_history_path(), job.id, &job.name, &r);
                (Some(r), None)
            }
            Err(e) => (None, Some(e.to_string())),
        };

        if notify {
            let body = match (&report, &error) {
                (Some(r), _) if r.ok() => format!(
                    "Xong: +{} mới, ~{} cập nhật, -{} xoá",
                    r.created, r.updated, r.deleted
                ),
                (Some(r), _) => format!("Kết thúc với {} lỗi", r.failed.len()),
                (None, Some(e)) => format!("Lỗi: {e}"),
                _ => String::new(),
            };
            let _ = app
                .notification()
                .builder()
                .title(format!("Backup: {}", job.name))
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

// ---------- Theo dõi cắm/rút USB ----------

fn watcher(app: AppHandle) {
    let mut present: HashSet<Uuid> = HashSet::new();
    let mut last_mounts: Vec<PathBuf> = vec![];
    loop {
        let drives = list_drives();
        let mounts: Vec<PathBuf> = drives.iter().map(|d| d.mount_point.clone()).collect();
        let now: HashSet<Uuid> = drives
            .iter()
            .filter_map(|d| d.marker.as_ref().map(|m| m.id))
            .collect();

        if mounts != last_mounts || now != present {
            let _ = app.emit("drives-changed", &drives);
        }

        let (jobs, poll) = {
            let cfg = app.state::<AppState>().config.lock().unwrap().clone();
            (cfg.jobs, cfg.poll_interval_secs.max(1))
        };
        for id in now.difference(&present) {
            for job in jobs.iter().filter(|j| j.target_id == *id && j.auto_run) {
                if let Err(e) = spawn_job(&app, job.clone(), false, true) {
                    tracing::warn!("auto-run {} lỗi: {e}", job.name);
                }
            }
        }

        present = now;
        last_mounts = mounts;
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
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let cfg_path = default_config_path();
    let config = Config::load(&cfg_path).unwrap_or_else(|e| {
        tracing::error!("Không đọc được config, dùng mặc định: {e}");
        Config::default()
    });

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
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            get_drives,
            mark_target,
            save_job,
            delete_job,
            start_job,
            cancel_job,
            running_jobs,
            get_history,
        ])
        .setup(|app| {
            let show = MenuItem::with_id(app, "show", "Mở USB Backup", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Thoát", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let mut tray = TrayIconBuilder::with_id("main")
                .tooltip("USB Backup")
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
