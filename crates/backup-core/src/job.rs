//! Ghép config + drives + plan + engine thành một lượt chạy job.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::config::Job;
use crate::drives::find_target;
use crate::engine::{execute, prune_versions, Progress, Report, RunOptions};
use crate::error::{BackupError, Result};
use crate::names::is_forbidden_char;
use crate::plan::{build_plan, Plan, PlanOptions};

/// Thư mục đích của job trên ổ USB.
pub fn dest_dir(job: &Job, target_root: &Path) -> PathBuf {
    let sub = if job.dest_subdir.trim().is_empty() {
        safe_dir_name(&job.name)
    } else {
        job.dest_subdir.clone()
    };
    target_root.join(sub)
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

/// Tìm ổ đích đang cắm của job.
pub fn locate_target(job: &Job) -> Result<PathBuf> {
    find_target(job.target_id)
        .map(|d| d.mount_point)
        .ok_or(BackupError::TargetNotFound(job.target_id))
}

pub fn plan_job(job: &Job, target_root: &Path) -> Result<Plan> {
    build_plan(&job.source, &dest_dir(job, target_root), &plan_options(job))
}

/// Chạy job vào `target_root` (đã biết). Dùng trực tiếp trong test,
/// hoặc khi người dùng chọn thư mục đích thủ công.
pub fn run_job_at(
    job: &Job,
    target_root: &Path,
    dry_run: bool,
    on_progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<Report> {
    let dst = dest_dir(job, target_root);
    let plan = build_plan(&job.source, &dst, &plan_options(job))?;
    let opts = RunOptions {
        dry_run,
        versioning: job.versioning,
        check_space: true,
    };
    let mut report = execute(&plan, &job.source, &dst, &opts, on_progress, cancel)?;

    if !dry_run && report.ok() {
        if let Some(days) = job.keep_versions_days {
            match prune_versions(&dst, days) {
                Ok(0) => {}
                Ok(n) => report.warnings.push(format!("Đã dọn {n} snapshot cũ hơn {days} ngày")),
                Err(e) => report.warnings.push(format!("Dọn .versions lỗi: {e}")),
            }
        }
    }
    Ok(report)
}

/// Tìm ổ đích theo marker rồi chạy.
pub fn run_job(
    job: &Job,
    dry_run: bool,
    on_progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<Report> {
    let root = locate_target(job)?;
    run_job_at(job, &root, dry_run, on_progress, cancel)
}
