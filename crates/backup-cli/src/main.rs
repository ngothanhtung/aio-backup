//! aio-backup CLI: test core, chạy headless hoặc gắn vào cron/Task Scheduler.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use backup_core::config::{default_config_path, Config, Destination, FtpTarget, Job, Schedule};
use backup_core::drives::list_drives;
use backup_core::engine::{Progress, Report};
use backup_core::history;
use backup_core::job::{locate_target, open_storage, run_job_on};
use backup_core::marker::{init_target, read_marker};
use backup_core::schedule::{Scheduler, Trigger};
use backup_core::secrets::{self, PASSWORD_ENV};
use backup_core::storage::{ftp::test_connection, Storage};
use clap::{Args, Parser, Subcommand};
use indicatif::{HumanBytes, ProgressBar, ProgressStyle};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "aio-backup", version, about = "AIO Backup: backup thư mục sang ổ USB hoặc FTP/FTPS")]
struct Cli {
    /// Đường dẫn file config (mặc định theo chuẩn từng OS)
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Liệt kê ổ đĩa đang mount
    Drives,
    /// Đánh dấu một ổ làm đích backup (tạo file .backup-id ở gốc ổ)
    InitTarget {
        path: PathBuf,
        #[arg(long, default_value = "Ổ backup")]
        label: String,
        #[arg(long)]
        force: bool,
    },
    /// Quản lý job
    #[command(subcommand)]
    Job(JobCmd),
    /// Chạy một job
    Run {
        job: String,
        #[arg(long)]
        dry_run: bool,
        /// (USB) Ghi vào thư mục này thay vì tự tìm ổ theo marker
        #[arg(long)]
        target_path: Option<PathBuf>,
    },
    /// Chạy mọi job: USB đang cắm và tất cả job FTP
    RunAll {
        #[arg(long)]
        dry_run: bool,
    },
    /// Chạy nền: cắm USB thì backup, job có lịch thì chạy định kỳ
    Watch,
    /// Xem lịch sử chạy
    History {
        #[arg(short, default_value_t = 10)]
        n: usize,
    },
}

#[derive(Subcommand)]
enum JobCmd {
    /// Thêm job. Đích là USB (--target) hoặc FTP (--ftp)
    Add(Box<AddJob>),
    List,
    Remove {
        job: String,
    },
    /// Đặt lại mật khẩu FTP của job
    SetPassword {
        job: String,
    },
    /// Kiểm tra kết nối và quyền ghi tới đích FTP
    Test {
        job: String,
    },
}

#[derive(Args)]
struct AddJob {
    #[arg(long)]
    name: String,
    #[arg(long)]
    source: PathBuf,
    /// USB: UUID của ổ đích, hoặc đường dẫn tới ổ đã init-target
    #[arg(long, conflicts_with = "ftp", required_unless_present = "ftp")]
    target: Option<String>,
    /// FTP: ftp://user@host[:port]/thu-muc  hoặc  ftps://... (FTPS explicit)
    #[arg(long)]
    ftp: Option<String>,
    /// FTP: chấp nhận chứng chỉ tự ký (NAS trong LAN)
    #[arg(long, requires = "ftp")]
    insecure: bool,
    /// FTP: mật khẩu (bỏ trống thì hỏi khi chạy lệnh, hoặc lấy từ biến AIO_BACKUP_FTP_PASSWORD)
    #[arg(long, requires = "ftp")]
    password: Option<String>,
    /// Chạy định kỳ mỗi N phút (mặc định FTP: 60)
    #[arg(long)]
    every: Option<u32>,
    /// Chạy vào giờ cố định HH:MM (giờ máy). Mặc định hằng ngày; thêm --weekdays hoặc --month-day
    #[arg(long, value_parser = parse_hhmm)]
    at: Option<chrono::NaiveTime>,
    /// Cùng --at: chỉ chạy các thứ này, 1 = thứ Hai ... 7 = Chủ nhật, vd 1,3,5
    #[arg(long, requires = "at", conflicts_with = "month_day", value_delimiter = ',', value_parser = clap::value_parser!(u8).range(1..=7))]
    weekdays: Vec<u8>,
    /// Cùng --at: chỉ chạy ngày này hằng tháng (tháng thiếu ngày thì lấy ngày cuối tháng)
    #[arg(long, requires = "at", value_parser = clap::value_parser!(u8).range(1..=31))]
    month_day: Option<u8>,
    #[arg(long, default_value = "")]
    subdir: String,
    /// Glob loại trừ, lặp lại được: --exclude node_modules --exclude "*.tmp"
    #[arg(long)]
    exclude: Vec<String>,
    /// Xoá ở đích file đã bị xoá ở nguồn
    #[arg(long)]
    mirror: bool,
    #[arg(long)]
    no_versioning: bool,
    /// Giữ snapshot .versions trong N ngày (0 = giữ mãi)
    #[arg(long, default_value_t = 30)]
    keep_days: u32,
    /// So blake3 khi size bằng nhau nhưng mtime khác (chỉ USB)
    #[arg(long)]
    hash: bool,
    /// Không tự chạy khi cắm USB
    #[arg(long)]
    no_auto: bool,
}

fn parse_hhmm(s: &str) -> Result<chrono::NaiveTime, String> {
    chrono::NaiveTime::parse_from_str(s, "%H:%M").map_err(|_| format!("'{s}' không đúng dạng HH:MM"))
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn,suppaftp=off".into()),
        )
        .init();

    let cli = Cli::parse();
    let cfg_path = cli.config.clone().unwrap_or_else(default_config_path);

    let cancel = Arc::new(AtomicBool::new(false));
    {
        let c = cancel.clone();
        ctrlc::set_handler(move || {
            eprintln!("\nĐang dừng sau file hiện tại...");
            c.store(true, Ordering::Relaxed);
        })
        .ok();
    }

    match cli.cmd {
        Cmd::Drives => cmd_drives(),
        Cmd::InitTarget { path, label, force } => {
            let m = init_target(&path, &label, force)?;
            println!("Đã đánh dấu {} làm ổ đích", path.display());
            println!("  id:    {}", m.id);
            println!("  label: {}", m.label);
            Ok(())
        }
        Cmd::Job(j) => cmd_job(j, &cfg_path),
        Cmd::Run {
            job,
            dry_run,
            target_path,
        } => {
            let cfg = Config::load(&cfg_path)?;
            let job = find(&cfg, &job)?;
            let r = match target_path {
                Some(root) => run_usb_at(&job, &root, dry_run, &cancel)?,
                None => run_one(&job, dry_run, &cancel)?,
            };
            if !r.ok() {
                std::process::exit(2);
            }
            Ok(())
        }
        Cmd::RunAll { dry_run } => {
            let cfg = Config::load(&cfg_path)?;
            let mut ran = 0;
            for job in &cfg.jobs {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                if matches!(job.destination, Destination::Usb { .. }) && locate_target(job).is_err() {
                    println!("- {}: ổ đích chưa cắm, bỏ qua", job.name);
                    continue;
                }
                match run_one(job, dry_run, &cancel) {
                    Ok(_) => ran += 1,
                    Err(e) => println!("  ✗ {}: {e:#}", job.name),
                }
            }
            println!("Đã chạy {ran}/{} job", cfg.jobs.len());
            Ok(())
        }
        Cmd::Watch => cmd_watch(&cfg_path, &cancel),
        Cmd::History { n } => {
            for e in history::recent(&history::default_history_path(), n) {
                let r = &e.report;
                if let Some(err) = r["error"].as_str() {
                    println!("{}  {:<20} ✗ {err}", r["startedAt"].as_str().unwrap_or("?"), e.job_name);
                    continue;
                }
                println!(
                    "{}  {:<20} +{} ~{} -{}  lỗi:{}{}",
                    r["startedAt"].as_str().unwrap_or("?"),
                    e.job_name,
                    r["created"],
                    r["updated"],
                    r["deleted"],
                    r["failed"].as_array().map_or(0, |a| a.len()),
                    if r["dryRun"].as_bool() == Some(true) { "  (dry-run)" } else { "" },
                );
            }
            Ok(())
        }
    }
}

fn find(cfg: &Config, key: &str) -> Result<Job> {
    cfg.find_job(key)
        .cloned()
        .with_context(|| format!("Không có job '{key}'"))
}

fn cmd_drives() -> Result<()> {
    for d in list_drives() {
        let tag = match &d.marker {
            Some(m) => format!("  [ĐÍCH: {} · {}]", m.label, m.id),
            None => String::new(),
        };
        println!(
            "{:<30} {:<8} {:>10} trống / {:>10}{}{}",
            d.mount_point.display(),
            d.file_system,
            HumanBytes(d.available_space).to_string(),
            HumanBytes(d.total_space).to_string(),
            if d.removable { "  (rời)" } else { "" },
            tag
        );
    }
    Ok(())
}

/// Phân tích `ftp[s]://user[:pass]@host[:port]/dir`.
fn parse_ftp_url(url: &str) -> Result<(FtpTarget, Option<String>)> {
    let (secure, rest) = if let Some(r) = url.strip_prefix("ftps://") {
        (true, r)
    } else if let Some(r) = url.strip_prefix("ftp://") {
        (false, r)
    } else {
        bail!("URL FTP phải bắt đầu bằng ftp:// hoặc ftps://");
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let (userinfo, hostport) = authority
        .rsplit_once('@')
        .context("Thiếu tên đăng nhập: ftp://user@host/...")?;
    let (username, password) = match userinfo.split_once(':') {
        Some((u, p)) => (u.to_string(), Some(p.to_string())),
        None => (userinfo.to_string(), None),
    };
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().context("Cổng không hợp lệ")?),
        None => (hostport.to_string(), 21),
    };
    if host.is_empty() || username.is_empty() {
        bail!("URL FTP thiếu host hoặc user");
    }
    Ok((
        FtpTarget {
            host,
            port,
            username,
            remote_dir: path.trim_end_matches('/').to_string(),
            secure,
            accept_invalid_certs: false,
            password: None,
        },
        password,
    ))
}

fn ask_password(prompt: &str) -> Result<String> {
    if let Ok(p) = std::env::var(PASSWORD_ENV) {
        if !p.is_empty() {
            return Ok(p);
        }
    }
    let p = rpassword::prompt_password(prompt).context("Không đọc được mật khẩu")?;
    if p.is_empty() {
        bail!("Mật khẩu trống");
    }
    Ok(p)
}

fn cmd_job(cmd: JobCmd, cfg_path: &Path) -> Result<()> {
    let mut cfg = Config::load(cfg_path)?;
    match cmd {
        JobCmd::Add(a) => {
            let a = *a;
            if cfg.find_job(&a.name).is_some() {
                bail!("Job '{}' đã tồn tại", a.name);
            }
            let source = std::fs::canonicalize(&a.source)
                .with_context(|| format!("Không thấy thư mục nguồn {}", a.source.display()))?;

            let mut password = None;
            let destination = match (&a.target, &a.ftp) {
                (Some(t), None) => {
                    let target_id = match Uuid::parse_str(t) {
                        Ok(id) => id,
                        Err(_) => {
                            read_marker(Path::new(t))
                                .with_context(|| format!("'{t}' không phải UUID, cũng chưa được init-target"))?
                                .id
                        }
                    };
                    Destination::Usb { target_id }
                }
                (None, Some(url)) => {
                    let (mut ftp, url_pw) = parse_ftp_url(url)?;
                    ftp.accept_invalid_certs = a.insecure;
                    if url_pw.is_some() {
                        eprintln!("! Không nên để mật khẩu trong URL (lộ trong lịch sử shell)");
                    }
                    password = Some(match a.password.clone().or(url_pw) {
                        Some(p) => p,
                        None => ask_password(&format!("Mật khẩu FTP cho {}: ", ftp.url()))?,
                    });
                    Destination::Ftp(ftp)
                }
                _ => bail!("Chọn một đích: --target (USB) hoặc --ftp"),
            };

            let mut job = Job::new(a.name, source, destination);
            job.dest_subdir = a.subdir;
            job.excludes = a.exclude;
            job.mirror_deletes = a.mirror;
            job.versioning = !a.no_versioning;
            job.keep_versions_days = (a.keep_days > 0).then_some(a.keep_days);
            job.verify_hash = a.hash;
            job.auto_run = !a.no_auto;
            if a.every.is_some() {
                job.interval_minutes = a.every.filter(|m| *m > 0);
            }
            if let Some(at) = a.at {
                job.schedule = Some(match (a.month_day, a.weekdays.is_empty()) {
                    (Some(day), _) => Schedule::Monthly { at, day },
                    (None, false) => Schedule::Weekly {
                        at,
                        weekdays: a.weekdays,
                    },
                    (None, true) => Schedule::Daily { at },
                });
                // Đã hẹn giờ thì bỏ mặc định "mỗi 60 phút" của FTP, trừ khi có --every
                if a.every.is_none() {
                    job.interval_minutes = None;
                }
            }

            if let Some(pw) = password {
                if let Some(warn) = secrets::set_job_password(&mut job, &pw) {
                    eprintln!("! {warn}");
                }
                match test_connection(job.destination.ftp().unwrap(), &pw) {
                    Ok(msg) => println!("✓ {msg}"),
                    Err(e) => eprintln!("! Chưa kết nối được ({e}). Job vẫn được lưu, sửa rồi chạy `job test`."),
                }
            }

            println!("Đã thêm job '{}' → {}", job.name, job.destination_label());
            cfg.jobs.push(job);
            cfg.save(cfg_path)?;
            println!("Config: {}", cfg_path.display());
        }
        JobCmd::List => {
            if cfg.jobs.is_empty() {
                println!("Chưa có job. Thêm: aio-backup job add --name ... --source ... --target <ổ USB> | --ftp ftp://user@host/dir");
            }
            for j in &cfg.jobs {
                let status = match &j.destination {
                    Destination::Usb { .. } => {
                        if locate_target(j).is_ok() {
                            "● đang cắm"
                        } else {
                            "○ chưa cắm"
                        }
                    }
                    Destination::Ftp(_) => "◆ FTP",
                };
                let every = j
                    .interval_minutes
                    .map(|m| format!(" · mỗi {m} phút"))
                    .unwrap_or_default()
                    + &j.schedule.as_ref().map(|s| format!(" · {s}")).unwrap_or_default();
                println!(
                    "{status}  {}\n    nguồn: {}\n    đích:  {}{every}\n    mirror:{} versioning:{} auto:{} exclude:{:?}",
                    j.name,
                    j.source.display(),
                    j.destination_label(),
                    j.mirror_deletes,
                    j.versioning,
                    j.auto_run,
                    j.excludes
                );
            }
        }
        JobCmd::Remove { job } => {
            let j = cfg.remove_job(&job)?;
            secrets::delete_password(j.id);
            cfg.save(cfg_path)?;
            println!("Đã xoá job '{}' (dữ liệu ở đích giữ nguyên)", j.name);
        }
        JobCmd::SetPassword { job } => {
            let key = find(&cfg, &job)?.id;
            let j = cfg.jobs.iter_mut().find(|j| j.id == key).unwrap();
            let Some(ftp) = j.destination.ftp() else {
                bail!("Job '{}' không dùng FTP", j.name);
            };
            let pw = ask_password(&format!("Mật khẩu mới cho {}: ", ftp.url()))?;
            if let Some(warn) = secrets::set_job_password(j, &pw) {
                eprintln!("! {warn}");
            }
            cfg.save(cfg_path)?;
            println!("Đã lưu mật khẩu");
        }
        JobCmd::Test { job } => {
            let j = find(&cfg, &job)?;
            let Some(ftp) = j.destination.ftp() else {
                println!(
                    "{}",
                    match locate_target(&j) {
                        Ok(p) => format!("✓ Ổ đích đang cắm tại {}", p.display()),
                        Err(e) => format!("✗ {e}"),
                    }
                );
                return Ok(());
            };
            let pw = secrets::load_password(&j).context("Chưa có mật khẩu, chạy `job set-password`")?;
            println!("✓ {}", test_connection(ftp, &pw)?);
        }
    }
    Ok(())
}

fn run_one(job: &Job, dry_run: bool, cancel: &AtomicBool) -> Result<Report> {
    let started_at = chrono::Utc::now();
    let mut storage = match open_storage(job) {
        Ok(s) => s,
        Err(e) => {
            let _ = history::append_error(
                &history::default_history_path(),
                job.id,
                &job.name,
                started_at,
                dry_run,
                &e.to_string(),
            );
            return Err(e.into());
        }
    };
    run_with(job, storage.as_mut(), dry_run, cancel)
}

fn run_usb_at(job: &Job, root: &Path, dry_run: bool, cancel: &AtomicBool) -> Result<Report> {
    if job.destination.ftp().is_some() {
        bail!("--target-path chỉ dùng cho job USB");
    }
    let mut storage = backup_core::storage::LocalStorage::new(backup_core::job::dest_dir(job, root));
    run_with(job, &mut storage, dry_run, cancel)
}

fn run_with(job: &Job, storage: &mut dyn Storage, dry_run: bool, cancel: &AtomicBool) -> Result<Report> {
    println!(
        "▶ {}: {} → {}{}",
        job.name,
        job.source.display(),
        storage.describe(),
        if dry_run { "  (dry-run)" } else { "" }
    );

    let bar = ProgressBar::hidden();
    bar.set_style(
        ProgressStyle::with_template("  {bar:30} {bytes}/{total_bytes} {msg}")
            .unwrap()
            .progress_chars("█▉▊▋▌▍▎▏ "),
    );
    let report = run_job_on(
        job,
        storage,
        dry_run,
        &mut |p| match p {
            Progress::Started { total_bytes, .. } => {
                bar.set_length(total_bytes);
                bar.set_draw_target(indicatif::ProgressDrawTarget::stderr());
            }
            Progress::File {
                rel, kind, done_bytes, ..
            } => {
                bar.set_position(done_bytes);
                if dry_run {
                    bar.suspend(|| println!("  {kind:?} {rel}"));
                } else {
                    bar.set_message(rel);
                }
            }
            Progress::Warning { message } => bar.suspend(|| eprintln!("  ! {message}")),
        },
        cancel,
    )?;
    bar.finish_and_clear();

    let secs = (report.finished_at - report.started_at).num_milliseconds() as f64 / 1000.0;
    println!(
        "  +{} mới, ~{} cập nhật, -{} xoá, ={} không đổi · {} · {:.1}s",
        report.created,
        report.updated,
        report.deleted,
        report.unchanged,
        HumanBytes(report.bytes_copied),
        secs
    );
    for f in &report.failed {
        println!("  ✗ {}: {}", f.rel, f.error);
    }
    if report.target_lost {
        println!("  ✗ Mất kết nối tới đích giữa chừng");
    }
    if report.cancelled {
        println!("  Đã huỷ");
    }
    for w in report.warnings.iter().filter(|w| w.starts_with("Đã dọn")) {
        println!("  {w}");
    }
    if let Err(e) = history::append(&history::default_history_path(), job.id, &job.name, &report) {
        eprintln!("  (không ghi được lịch sử: {e})");
    }
    Ok(report)
}

fn cmd_watch(cfg_path: &Path, cancel: &AtomicBool) -> Result<()> {
    let mut scheduler = Scheduler::new(history::last_runs(&history::default_history_path()));
    println!("Đang theo dõi... cắm USB đích để backup, job có lịch chạy định kỳ (Ctrl+C để thoát)");
    while !cancel.load(Ordering::Relaxed) {
        // Đọc lại config mỗi vòng để nhận job mới mà không cần restart
        let cfg = Config::load(cfg_path)?;
        let present: HashSet<Uuid> = list_drives()
            .into_iter()
            .filter_map(|d| d.marker.map(|m| m.id))
            .collect();

        for due in scheduler.tick(&cfg.jobs, &present, chrono::Utc::now()) {
            let why = match due.trigger {
                Trigger::Plugged => "ổ đích vừa cắm",
                Trigger::Interval => "tới lịch",
            };
            println!("[{}] {why}", chrono::Local::now().format("%H:%M:%S"));
            if let Err(e) = run_one(&due.job, false, cancel) {
                eprintln!("  ✗ {}: {e:#}", due.job.name);
            }
        }
        std::thread::sleep(Duration::from_secs(cfg.poll_interval_secs.max(1)));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_ftp_url;

    #[test]
    fn ftp_url() {
        let (t, p) = parse_ftp_url("ftps://backup@192.168.1.10:2121/volume1/backup/").unwrap();
        assert!(t.secure);
        assert_eq!((t.host.as_str(), t.port, t.username.as_str()), ("192.168.1.10", 2121, "backup"));
        assert_eq!(t.remote_dir, "/volume1/backup");
        assert!(p.is_none());

        let (t, p) = parse_ftp_url("ftp://u:s3cr3t@nas.local").unwrap();
        assert_eq!((t.port, t.remote_dir.as_str(), p.as_deref()), (21, "", Some("s3cr3t")));

        assert!(parse_ftp_url("sftp://u@h").is_err());
        assert!(parse_ftp_url("ftp://host-only").is_err());
    }
}
