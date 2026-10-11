//! Khôi phục file từ bản backup hiện tại hoặc từ một snapshot trong `.versions`.
//! Dùng chung cho USB và FTP qua trait [`Storage`].

use std::collections::HashMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use filetime::FileTime;
use serde::Serialize;

use crate::engine::{Failure, VERSION_FMT};
use crate::error::{BackupError, IoContext, Result};
use crate::scan::Filter;
use crate::storage::{RemoteFile, Storage};
use crate::{TMP_SUFFIX, VERSIONS_DIR};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotInfo {
    /// Tên thư mục, vd `2026-10-11_172143`.
    pub name: String,
    /// Thời điểm tạo (giờ local, RFC 3339).
    pub taken_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreEntry {
    /// Đường dẫn tương đối, phân cách bằng `/`.
    pub path: String,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RestoreReport {
    pub restored: usize,
    pub bytes: u64,
    /// File đã có ở thư mục đích và không chọn ghi đè.
    pub skipped: Vec<String>,
    pub failed: Vec<Failure>,
    pub cancelled: bool,
}

/// Snapshot mới nhất trước.
pub fn list_snapshots(storage: &mut dyn Storage) -> Result<Vec<SnapshotInfo>> {
    let mut names = storage.list_snapshots()?;
    names.sort_by(|a, b| b.cmp(a));
    Ok(names
        .into_iter()
        .map(|name| {
            let taken_at = NaiveDateTime::parse_from_str(&name, VERSION_FMT)
                .ok()
                .and_then(|n| Local.from_local_datetime(&n).single())
                .map(|t| t.to_rfc3339());
            SnapshotInfo { name, taken_at }
        })
        .collect())
}

/// Danh sách file có thể khôi phục. `snapshot = None` là bản backup hiện tại.
pub fn list_entries(storage: &mut dyn Storage, snapshot: Option<&str>) -> Result<Vec<RestoreEntry>> {
    let files: Vec<RemoteFile> = match snapshot {
        Some(name) => {
            check_snapshot(name)?;
            storage.list_files_under(&Path::new(VERSIONS_DIR).join(name))?
        }
        None => {
            // index() đã loại file nội bộ (.versions, manifest, file tạm)
            let idx = storage.index(&Filter::new(&[])?)?;
            idx.files
                .into_iter()
                .map(|(rel, e)| RemoteFile {
                    rel,
                    size: e.size,
                    mtime: (e.mtime > SystemTime::UNIX_EPOCH).then_some(e.mtime),
                })
                .collect()
        }
    };
    let mut out: Vec<RestoreEntry> = files
        .into_iter()
        .filter(|f| !f.rel.to_string_lossy().ends_with(TMP_SUFFIX))
        .map(|f| RestoreEntry {
            path: slash(&f.rel),
            size: f.size,
            modified: f.mtime.map(DateTime::<Utc>::from),
        })
        .collect();
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct RestoreOptions {
    /// Thư mục nhận file khôi phục; cấu trúc thư mục con được giữ nguyên.
    pub target: PathBuf,
    /// Ghi đè file đã có ở thư mục nhận.
    pub overwrite: bool,
}

/// Khôi phục `paths` (đường dẫn tương đối dạng `a/b.txt`) về `opts.target`.
/// `on_progress(xong, tổng, file hiện tại)`.
pub fn restore(
    storage: &mut dyn Storage,
    snapshot: Option<&str>,
    paths: &[String],
    opts: &RestoreOptions,
    on_progress: &mut dyn FnMut(usize, usize, &str),
    cancel: &AtomicBool,
) -> Result<RestoreReport> {
    if let Some(s) = snapshot {
        check_snapshot(s)?;
    }
    fs::create_dir_all(&opts.target).at(&opts.target)?;

    // Lấy mtime từ danh sách để đặt lại cho file khôi phục (nếu biết)
    let known: HashMap<String, RestoreEntry> = list_entries(storage, snapshot)?
        .into_iter()
        .map(|e| (e.path.clone(), e))
        .collect();

    let mut report = RestoreReport::default();
    let total = paths.len();
    for (i, p) in paths.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        on_progress(i, total, p);

        let Some(rel) = safe_rel(p) else {
            report.failed.push(Failure {
                rel: p.clone(),
                error: "Đường dẫn không hợp lệ".into(),
            });
            continue;
        };
        let dest = opts.target.join(&rel);
        if dest.exists() && !opts.overwrite {
            report.skipped.push(p.clone());
            continue;
        }

        let src_rel = match snapshot {
            Some(s) => Path::new(VERSIONS_DIR).join(s).join(&rel),
            None => rel.clone(),
        };
        match fetch_atomic(storage, &src_rel, &dest) {
            Ok(()) => {
                let entry = known.get(p);
                if let Some(t) = entry.and_then(|e| e.modified) {
                    let _ = filetime::set_file_mtime(&dest, FileTime::from_system_time(t.into()));
                }
                report.restored += 1;
                report.bytes += entry.map_or(0, |e| e.size);
            }
            Err(e) => {
                report.failed.push(Failure {
                    rel: p.clone(),
                    error: e.to_string(),
                });
                if !storage.is_alive() {
                    break;
                }
            }
        }
    }
    on_progress(total, total, "");
    Ok(report)
}

fn fetch_atomic(storage: &mut dyn Storage, src_rel: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).at(parent)?;
    }
    let mut tmp = dest.as_os_str().to_os_string();
    tmp.push(TMP_SUFFIX);
    let tmp = PathBuf::from(tmp);
    let res = storage
        .get(src_rel, &tmp)
        .and_then(|_| fs::rename(&tmp, dest).at(dest));
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

/// Chỉ chấp nhận đường dẫn tương đối thuần (không `..`, không tuyệt đối),
/// để tên file lấy từ server không ghi được ra ngoài thư mục nhận.
fn safe_rel(p: &str) -> Option<PathBuf> {
    let path = PathBuf::from(p.replace('\\', "/"));
    let ok = !p.is_empty()
        && path
            .components()
            .all(|c| matches!(c, Component::Normal(_)));
    ok.then_some(path)
}

fn check_snapshot(name: &str) -> Result<()> {
    if crate::storage::is_snapshot_name(name) {
        Ok(())
    } else {
        Err(BackupError::Remote(format!("Snapshot không hợp lệ: {name}")))
    }
}

fn slash(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal() {
        assert!(safe_rel("a/b.txt").is_some());
        assert!(safe_rel("../etc/passwd").is_none());
        assert!(safe_rel("/etc/passwd").is_none());
        assert!(safe_rel("a/../../x").is_none());
        assert!(safe_rel("").is_none());
    }
}
