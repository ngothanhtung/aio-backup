//! usb-backup CLI: dùng để test core, chạy headless hoặc gắn vào cron/Task Scheduler.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use backup_core::config::{default_config_path, Config, Job};
use backup_core::drives::list_drives;
use backup_core::engine::{Progress, Report};
use backup_core::history;
use backup_core::job::{dest_dir, locate_target, run_job_at};
use backup_core::marker::{init_target, read_marker};
use clap::{Args, Parser, Subcommand};
use indicatif::{HumanBytes, ProgressBar, ProgressStyle};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "usb-backup", version, about = "Backup thư mục sang ổ USB")]
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
    /// Đánh dấu một ổ/thư mục làm đích backup (tạo file .backup-id)
    InitTarget {
        path: PathBuf,
        #[arg(long, default_value = "USB Backup")]
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
        /// Ghi vào thư mục này thay vì tự tìm ổ theo marker
        #[arg(long)]
        target_path: Option<PathBuf>,
    },
    /// Chạy mọi job có ổ đích đang cắm
    RunAll {
        #[arg(long)]
        dry_run: bool,
    },
    /// Chạy nền: phát hiện USB được cắm thì tự chạy các job auto_run
    Watch,
    /// Xem lịch sử chạy
    History {
        #[arg(short, default_value_t = 10)]
        n: usize,
    },
}

#[derive(Subcommand)]
enum JobCmd {
    Add(AddJob),
    List,
    Remove { job: String },
}

#[derive(Args)]
struct AddJob {
    #[arg(long)]
    name: String,
    #[arg(long)]
    source: PathBuf,
    /// UUID của ổ đích, hoặc đường dẫn tới ổ đã init-target
    #[arg(long)]
    target: String,
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
    /// So blake3 khi size bằng nhau nhưng mtime khác
    #[arg(long)]
    hash: bool,
    /// Không tự chạy khi cắm USB
    #[arg(long)]
    no_auto: bool,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
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
            let job = cfg
                .find_job(&job)
                .with_context(|| format!("Không có job '{job}'"))?
                .clone();
            let root = match target_path {
                Some(p) => p,
                None => locate_target(&job)?,
            };
            let r = run_one(&job, &root, dry_run, &cancel)?;
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
                match locate_target(job) {
                    Ok(root) => {
                        run_one(job, &root, dry_run, &cancel)?;
                        ran += 1;
                    }
                    Err(_) => println!("- {}: ổ đích chưa cắm, bỏ qua", job.name),
                }
            }
            println!("Đã chạy {ran}/{} job", cfg.jobs.len());
            Ok(())
        }
        Cmd::Watch => cmd_watch(&cfg_path, &cancel),
        Cmd::History { n } => {
            for e in history::recent(&history::default_history_path(), n) {
                let r = &e.report;
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

fn cmd_job(cmd: JobCmd, cfg_path: &Path) -> Result<()> {
    let mut cfg = Config::load(cfg_path)?;
    match cmd {
        JobCmd::Add(a) => {
            if cfg.find_job(&a.name).is_some() {
                bail!("Job '{}' đã tồn tại", a.name);
            }
            let source = std::fs::canonicalize(&a.source)
                .with_context(|| format!("Không thấy thư mục nguồn {}", a.source.display()))?;
            let target_id = match Uuid::parse_str(&a.target) {
                Ok(id) => id,
                Err(_) => {
                    read_marker(Path::new(&a.target))
                        .with_context(|| {
                            format!("'{}' không phải UUID, cũng chưa được init-target", a.target)
                        })?
                        .id
                }
            };
            let mut job = Job::new(a.name, source, target_id);
            job.dest_subdir = a.subdir;
            job.excludes = a.exclude;
            job.mirror_deletes = a.mirror;
            job.versioning = !a.no_versioning;
            job.keep_versions_days = (a.keep_days > 0).then_some(a.keep_days);
            job.verify_hash = a.hash;
            job.auto_run = !a.no_auto;
            println!("Đã thêm job '{}' ({})", job.name, job.id);
            cfg.jobs.push(job);
            cfg.save(cfg_path)?;
            println!("Config: {}", cfg_path.display());
        }
        JobCmd::List => {
            if cfg.jobs.is_empty() {
                println!("Chưa có job. Thêm: usb-backup job add --name ... --source ... --target ...");
            }
            for j in &cfg.jobs {
                let present = locate_target(j).is_ok();
                println!(
                    "{}  {}\n    nguồn: {}\n    đích:  {} {}\n    mirror:{} versioning:{} auto:{} exclude:{:?}",
                    if present { "●" } else { "○" },
                    j.name,
                    j.source.display(),
                    j.target_id,
                    if present { "(đang cắm)" } else { "(chưa cắm)" },
                    j.mirror_deletes,
                    j.versioning,
                    j.auto_run,
                    j.excludes
                );
            }
        }
        JobCmd::Remove { job } => {
            let j = cfg.remove_job(&job)?;
            cfg.save(cfg_path)?;
            println!("Đã xoá job '{}' (dữ liệu trên USB giữ nguyên)", j.name);
        }
    }
    Ok(())
}

fn run_one(job: &Job, root: &Path, dry_run: bool, cancel: &AtomicBool) -> Result<Report> {
    let dst = dest_dir(job, root);
    println!(
        "▶ {}: {} → {}{}",
        job.name,
        job.source.display(),
        dst.display(),
        if dry_run { "  (dry-run)" } else { "" }
    );

    let bar = ProgressBar::hidden();
    bar.set_style(
        ProgressStyle::with_template("  {bar:30} {bytes}/{total_bytes} {msg}")
            .unwrap()
            .progress_chars("█▉▊▋▌▍▎▏ "),
    );
    let report = run_job_at(
        job,
        root,
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
        println!("  ✗ Ổ đích bị ngắt kết nối giữa chừng");
    }
    if report.cancelled {
        println!("  Đã huỷ");
    }
    if let Err(e) = history::append(&history::default_history_path(), job.id, &job.name, &report) {
        eprintln!("  (không ghi được lịch sử: {e})");
    }
    Ok(report)
}

fn cmd_watch(cfg_path: &Path, cancel: &AtomicBool) -> Result<()> {
    let mut present: HashSet<Uuid> = HashSet::new();
    println!("Đang theo dõi ổ USB... (Ctrl+C để thoát)");
    while !cancel.load(Ordering::Relaxed) {
        // Đọc lại config mỗi vòng để nhận job mới mà không cần restart
        let cfg = Config::load(cfg_path)?;
        let now: HashSet<Uuid> = list_drives()
            .into_iter()
            .filter_map(|d| d.marker.map(|m| m.id))
            .collect();

        for id in now.difference(&present) {
            for job in cfg.jobs.iter().filter(|j| j.target_id == *id && j.auto_run) {
                if let Ok(root) = locate_target(job) {
                    if let Err(e) = run_one(job, &root, false, cancel) {
                        eprintln!("  ✗ {}: {e}", job.name);
                    }
                }
            }
        }
        present = now;
        std::thread::sleep(Duration::from_secs(cfg.poll_interval_secs.max(1)));
    }
    Ok(())
}
