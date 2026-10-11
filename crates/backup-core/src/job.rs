//! Ghép config + storage + plan + engine thành một lượt chạy job.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::config::{Destination, Job};
use crate::drives::find_target;
use crate::engine::{execute, Progress, Report, RunOptions};
use crate::error::{BackupError, Result};
use crate::names::is_forbidden_char;
use crate::plan::{build_plan_for, PlanOptions};
use crate::secrets::load_password;
use crate::storage::{FtpStorage, LocalStorage, Storage};

/// Tên thư mục con của job ở đích (USB hoặc FTP).
pub fn dest_subdir(job: &Job) -> String {
    if job.dest_subdir.trim().is_empty() {
        safe_dir_name(&job.name)
    } else {
        job.dest_subdir.trim().to_string()
    }
}

/// Thư mục đích của job USB trên ổ có gốc `target_root`.
pub fn dest_dir(job: &Job, target_root: &Path) -> PathBuf {
    target_root.join(dest_subdir(job))
}

fn safe_dir_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if is_forbidden_char(c) { '_' } else { c })
        .collect();
    let trimmed = cleaned.trim_end_matches(['.', ' ']).to_string();
    if trimmed.is_empty() {
        "backup".into()
    } else {
        trimmed
    }
}

pub fn plan_options(job: &Job) -> PlanOptions {
    PlanOptions {
        excludes: job.excludes.clone(),
        mirror_deletes: job.mirror_deletes,
        verify_hash: job.verify_hash,
    }
}

/// Gốc ổ USB đích đang cắm của job.
pub fn locate_target(job: &Job) -> Result<PathBuf> {
    match &job.destination {
        Destination::Usb { target_id } => find_target(*target_id)
            .map(|d| d.mount_point)
            .ok_or(BackupError::TargetNotFound(*target_id)),
        Destination::Ftp(_) => Err(BackupError::Remote("Job này backup qua FTP, không dùng ổ USB".into())),
    }
}

/// Mở đích của job: tìm ổ USB theo marker, hoặc kết nối FTP.
pub fn open_storage(job: &Job) -> Result<Box<dyn Storage>> {
    match &job.destination {
        Destination::Usb { .. } => {
            let root = locate_target(job)?;
            Ok(Box::new(LocalStorage::new(dest_dir(job, &root))))
        }
        Destination::Ftp(ftp) => {
            let password = load_password(job).ok_or(BackupError::MissingPassword)?;
            Ok(Box::new(FtpStorage::connect(ftp, &password, &dest_subdir(job))?))
        }
    }
}

/// Chạy job trên một storage đã mở.
pub fn run_job_on(
    job: &Job,
    storage: &mut dyn Storage,
    dry_run: bool,
    on_progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<Report> {
    let plan = build_plan_for(&job.source, storage, &plan_options(job))?;
    let opts = RunOptions {
        dry_run,
        versioning: job.versioning,
        check_space: true,
    };
    let mut report = execute(&plan, &job.source, storage, &opts, on_progress, cancel)?;

    if !dry_run && report.ok() {
        if let Some(days) = job.keep_versions_days.filter(|d| *d > 0) {
            match storage.prune_versions(days) {
                Ok(0) => {}
                Ok(n) => report.warnings.push(format!("Đã dọn {n} snapshot cũ hơn {days} ngày")),
                Err(e) => report.warnings.push(format!("Dọn .versions lỗi: {e}")),
            }
        }
    }
    Ok(report)
}

/// Chạy job USB vào `target_root` đã biết (test, hoặc chọn thư mục đích thủ công).
pub fn run_job_at(
    job: &Job,
    target_root: &Path,
    dry_run: bool,
    on_progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<Report> {
    let mut storage = LocalStorage::new(dest_dir(job, target_root));
    run_job_on(job, &mut storage, dry_run, on_progress, cancel)
}

/// Mở đích rồi chạy.
pub fn run_job(
    job: &Job,
    dry_run: bool,
    on_progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<Report> {
    let mut storage = open_storage(job)?;
    run_job_on(job, storage.as_mut(), dry_run, on_progress, cancel)
}
