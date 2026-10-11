//! Thực thi Plan: copy atomic, giữ mtime, versioning, huỷ giữa chừng.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use filetime::FileTime;
use serde::Serialize;
use walkdir::WalkDir;

use crate::drives::available_space_for;
use crate::error::{BackupError, IoContext, Result};
use crate::plan::{ActionKind, Plan};
use crate::{TMP_SUFFIX, VERSIONS_DIR};

const VERSION_FMT: &str = "%Y-%m-%d_%H%M%S";
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
    dst: &Path,
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
        fs::create_dir_all(dst).at(dst)?;
        cleanup_tmp(dst);
        if opts.check_space && plan.bytes_to_copy > 0 {
            if let Some(available) = available_space_for(dst) {
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

    let version_root = dst
        .join(VERSIONS_DIR)
        .join(Local::now().format(VERSION_FMT).to_string());
    let mut done_bytes = 0u64;

    for (i, action) in plan.actions.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        let from = src.join(&action.rel);
        let to = dst.join(&action.rel);

        let res = if opts.dry_run {
            Ok(())
        } else {
            match action.kind {
                ActionKind::Create => copy_atomic(&from, &to, None),
                ActionKind::Update => {
                    let keep = opts.versioning.then(|| version_root.join(&action.rel));
                    copy_atomic(&from, &to, keep.as_deref())
                }
                ActionKind::Delete => remove_or_version(
                    &to,
                    opts.versioning.then(|| version_root.join(&action.rel)).as_deref(),
                    dst,
                ),
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
                // Rút USB giữa chừng -> dừng ngay, không spam lỗi cho từng file
                if !dst.exists() {
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

    report.finished_at = Utc::now();
    Ok(report)
}

fn tmp_path(to: &Path) -> PathBuf {
    let mut name = to.file_name().unwrap_or_default().to_os_string();
    name.push(TMP_SUFFIX);
    to.with_file_name(name)
}

/// Copy vào file tạm, fsync, đặt mtime, rồi mới rename vào chỗ.
/// Nếu `keep_old` có giá trị, bản cũ được chuyển tới đó trước khi thay.
fn copy_atomic(from: &Path, to: &Path, keep_old: Option<&Path>) -> Result<()> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).at(parent)?;
    }
    let tmp = tmp_path(to);
    let src_md = fs::metadata(from).at(from)?;

    let res = (|| {
        fs::copy(from, &tmp).at(&tmp)?;
        fs::OpenOptions::new()
            .write(true)
            .open(&tmp)
            .and_then(|f| f.sync_all())
            .at(&tmp)?;
        filetime::set_file_mtime(&tmp, FileTime::from_last_modification_time(&src_md)).at(&tmp)?;

        if let Some(keep) = keep_old {
            if to.exists() {
                if let Some(p) = keep.parent() {
                    fs::create_dir_all(p).at(p)?;
                }
                fs::rename(to, keep).at(to)?;
            }
        }
        fs::rename(&tmp, to).at(to)
    })();

    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

fn remove_or_version(path: &Path, keep: Option<&Path>, root: &Path) -> Result<()> {
    match keep {
        Some(k) => {
            if let Some(p) = k.parent() {
                fs::create_dir_all(p).at(p)?;
            }
            fs::rename(path, k).at(path)?;
        }
        None => fs::remove_file(path).at(path)?,
    }
    // Dọn thư mục rỗng phía trên, dừng ở root
    let mut dir = path.parent();
    while let Some(d) = dir {
        if d == root || fs::remove_dir(d).is_err() {
            break;
        }
        dir = d.parent();
    }
    Ok(())
}

/// Xoá file tạm sót lại từ lần chạy bị ngắt (rút USB, mất điện).
fn cleanup_tmp(dst: &Path) {
    let walker = WalkDir::new(dst).into_iter().filter_entry(|e| {
        e.depth() != 1 || e.file_name() != VERSIONS_DIR
    });
    for e in walker.flatten() {
        if e.file_type().is_file() && e.file_name().to_string_lossy().ends_with(TMP_SUFFIX) {
            let _ = fs::remove_file(e.path());
        }
    }
}

/// Xoá các snapshot trong `.versions` cũ hơn `keep_days` ngày.
/// Trả về số snapshot đã xoá.
pub fn prune_versions(dst: &Path, keep_days: u32) -> Result<usize> {
    let root = dst.join(VERSIONS_DIR);
    let Ok(rd) = fs::read_dir(&root) else {
        return Ok(0);
    };
    let cutoff = Local::now() - chrono::Duration::days(keep_days as i64);
    let mut removed = 0;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(naive) = NaiveDateTime::parse_from_str(&name, VERSION_FMT) else {
            continue;
        };
        let Some(ts) = Local.from_local_datetime(&naive).single() else {
            continue;
        };
        if ts < cutoff {
            fs::remove_dir_all(e.path()).at(e.path())?;
            removed += 1;
        }
    }
    Ok(removed)
}
