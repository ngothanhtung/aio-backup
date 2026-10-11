//! Đích là thư mục local (ổ USB, ổ ngoài, thư mục mạng đã mount).

use std::fs;
use std::path::{Path, PathBuf};

use filetime::FileTime;
use walkdir::WalkDir;

use super::{is_snapshot_name, RemoteFile, Storage};
use crate::drives::available_space_for;
use crate::error::{IoContext, Result};
use crate::scan::{scan, Filter, ScanResult};
use crate::{TMP_SUFFIX, VERSIONS_DIR};

pub struct LocalStorage {
    root: PathBuf,
}

impl LocalStorage {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Storage for LocalStorage {
    fn describe(&self) -> String {
        self.root.display().to_string()
    }

    fn prepare(&mut self) -> Result<()> {
        fs::create_dir_all(&self.root).at(&self.root)?;
        cleanup_tmp(&self.root);
        Ok(())
    }

    fn index(&mut self, filter: &Filter) -> Result<ScanResult> {
        scan(&self.root, filter)
    }

    fn local_path(&self, rel: &Path) -> Option<PathBuf> {
        Some(self.root.join(rel))
    }

    fn available_space(&mut self) -> Option<u64> {
        // Thư mục job có thể chưa tồn tại (lần chạy đầu / dry-run) -> đo ở thư mục cha gần nhất
        let mut p = self.root.as_path();
        while !p.exists() {
            p = p.parent()?;
        }
        available_space_for(p)
    }

    fn put(&mut self, src: &Path, rel: &Path, keep_old: Option<&Path>) -> Result<()> {
        let keep = keep_old.map(|k| self.root.join(k));
        copy_atomic(src, &self.root.join(rel), keep.as_deref())
    }

    fn remove(&mut self, rel: &Path, keep: Option<&Path>) -> Result<()> {
        let keep = keep.map(|k| self.root.join(k));
        remove_or_version(&self.root.join(rel), keep.as_deref(), &self.root)
    }

    fn is_alive(&mut self) -> bool {
        self.root.exists()
    }

    fn finish(&mut self) -> Result<()> {
        Ok(())
    }

    fn list_snapshots(&mut self) -> Result<Vec<String>> {
        let Ok(rd) = fs::read_dir(self.root.join(VERSIONS_DIR)) else {
            return Ok(vec![]);
        };
        Ok(rd
            .flatten()
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| is_snapshot_name(n))
            .collect())
    }

    fn remove_snapshot(&mut self, name: &str) -> Result<()> {
        let p = self.root.join(VERSIONS_DIR).join(name);
        fs::remove_dir_all(&p).at(&p)
    }

    fn list_files_under(&mut self, rel_dir: &Path) -> Result<Vec<RemoteFile>> {
        let base = self.root.join(rel_dir);
        let mut out = vec![];
        for e in WalkDir::new(&base).follow_links(false).into_iter().flatten() {
            if !e.file_type().is_file() {
                continue;
            }
            let Ok(rel) = e.path().strip_prefix(&base) else {
                continue;
            };
            let md = e.metadata().ok();
            out.push(RemoteFile {
                rel: rel.to_path_buf(),
                size: md.as_ref().map_or(0, |m| m.len()),
                mtime: md.and_then(|m| m.modified().ok()),
            });
        }
        Ok(out)
    }

    fn get(&mut self, rel: &Path, dest: &Path) -> Result<()> {
        let src = self.root.join(rel);
        fs::copy(&src, dest).at(&src)?;
        Ok(())
    }
}

fn tmp_path(to: &Path) -> PathBuf {
    let mut name = to.file_name().unwrap_or_default().to_os_string();
    name.push(TMP_SUFFIX);
    to.with_file_name(name)
}

/// Copy vào file tạm, fsync, đặt mtime, rồi mới rename vào chỗ.
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
    let walker = WalkDir::new(dst)
        .into_iter()
        .filter_entry(|e| e.depth() != 1 || e.file_name() != VERSIONS_DIR);
    for e in walker.flatten() {
        if e.file_type().is_file() && e.file_name().to_string_lossy().ends_with(TMP_SUFFIX) {
            let _ = fs::remove_file(e.path());
        }
    }
}
