//! Thực thi Plan trên một [`Storage`]: versioning, huỷ giữa chừng,
//! dừng khi mất đích (rút USB / rớt kết nối FTP).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, Local, Utc};
use serde::Serialize;

use crate::error::{BackupError, Result};
use crate::plan::{ActionKind, Plan};
use crate::storage::Storage;
use crate::VERSIONS_DIR;

/// Định dạng tên snapshot trong `.versions/`.
pub const VERSION_FMT: &str = "%Y-%m-%d_%H%M%S";
/// Chừa lại 64 MB trống để không làm đầy ổ hoàn toàn.
const SPACE_MARGIN: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct RunOptions {
    pub dry_run: bool,
    /// File bị ghi đè/xoá được chuyển vào `.versions/<thời điểm>/` thay vì mất.
    pub versioning: bool,
    pub check_space: bool,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            versioning: true,
            check_space: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Progress {
    #[serde(rename_all = "camelCase")]
    Started { total_files: usize, total_bytes: u64 },
    #[serde(rename_all = "camelCase")]
    File {
        rel: String,
        kind: ActionKind,
        done_files: usize,
        done_bytes: u64,
    },
    Warning { message: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub rel: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    pub dry_run: bool,
    pub created: usize,
    pub updated: usize,
    pub deleted: usize,
    pub unchanged: usize,
    pub bytes_copied: u64,
    pub failed: Vec<Failure>,
    pub warnings: Vec<String>,
    pub cancelled: bool,
    /// Ổ đích biến mất giữa chừng (rút USB).
    pub target_lost: bool,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.failed.is_empty() && !self.cancelled && !self.target_lost
    }
}

pub fn execute(
    plan: &Plan,
    src: &Path,
    storage: &mut dyn Storage,
    opts: &RunOptions,
    on_progress: &mut dyn FnMut(Progress),
    cancel: &AtomicBool,
) -> Result<Report> {
    let started_at = Utc::now();
    let mut report = Report {
        started_at,
        finished_at: started_at,
        dry_run: opts.dry_run,
        created: 0,
        updated: 0,
        deleted: 0,
        unchanged: plan.unchanged,
        bytes_copied: 0,
        failed: vec![],
        warnings: plan.warnings.clone(),
        cancelled: false,
        target_lost: false,
    };
    for w in &plan.warnings {
        on_progress(Progress::Warning { message: w.clone() });
    }

    if !opts.dry_run {
        storage.prepare()?;
        if opts.check_space && plan.bytes_to_copy > 0 {
            if let Some(available) = storage.available_space() {
                let needed = plan.bytes_to_copy + SPACE_MARGIN;
                if needed > available {
                    return Err(BackupError::InsufficientSpace { needed, available });
                }
            }
        }
    }

    on_progress(Progress::Started {
        total_files: plan.actions.len(),
        total_bytes: plan.bytes_to_copy,
    });

    let version_root = PathBuf::from(VERSIONS_DIR).join(Local::now().format(VERSION_FMT).to_string());
    let mut done_bytes = 0u64;

    for (i, action) in plan.actions.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        let from = src.join(&action.rel);
        let keep = opts.versioning.then(|| version_root.join(&action.rel));

        let res = if opts.dry_run {
            Ok(())
        } else {
            match action.kind {
                ActionKind::Create => storage.put(&from, &action.rel, None),
                ActionKind::Update => storage.put(&from, &action.rel, keep.as_deref()),
                ActionKind::Delete => storage.remove(&action.rel, keep.as_deref()),
            }
        };

        match res {
            Ok(()) => {
                match action.kind {
                    ActionKind::Create => report.created += 1,
                    ActionKind::Update => report.updated += 1,
                    ActionKind::Delete => report.deleted += 1,
                }
                if action.kind != ActionKind::Delete {
                    done_bytes += action.size;
                    report.bytes_copied += action.size;
                }
            }
            Err(e) => {
                report.failed.push(Failure {
                    rel: action.rel.display().to_string(),
                    error: e.to_string(),
                });
                // Rút USB / mất kết nối -> dừng ngay, không spam lỗi cho từng file
                if !storage.is_alive() {
                    report.target_lost = true;
                    break;
                }
            }
        }

        on_progress(Progress::File {
            rel: action.rel.display().to_string(),
            kind: action.kind,
            done_files: i + 1,
            done_bytes,
        });
    }

    if !opts.dry_run && !report.target_lost {
        if let Err(e) = storage.finish() {
            report.warnings.push(format!("Không lưu được trạng thái ở đích: {e}"));
        }
    }
    report.finished_at = Utc::now();
    Ok(report)
}
