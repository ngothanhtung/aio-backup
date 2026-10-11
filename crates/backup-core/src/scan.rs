//! Duyệt cây thư mục, trả về map đường dẫn tương đối -> metadata.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use globset::{Glob, GlobSet, GlobSetBuilder};
use walkdir::WalkDir;

use crate::error::{BackupError, Result};
use crate::{MARKER_FILE, TMP_SUFFIX, VERSIONS_DIR};

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub size: u64,
    pub mtime: SystemTime,
}

#[derive(Debug, Default)]
pub struct ScanResult {
    /// Key là đường dẫn tương đối so với root.
    pub files: BTreeMap<PathBuf, FileEntry>,
    pub warnings: Vec<String>,
}

/// Bộ lọc exclude dạng glob, so khớp trên đường dẫn tương đối dùng `/`.
#[derive(Debug, Clone)]
pub struct Filter {
    set: GlobSet,
}

/// Luôn bỏ qua: file hệ thống/rác thường gặp trên 3 OS.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    "**/.DS_Store",
    "**/._*",
    "**/Thumbs.db",
    "**/desktop.ini",
    "**/$RECYCLE.BIN/**",
    "**/System Volume Information/**",
    "**/.Trash-*/**",
    "**/.Spotlight-V100/**",
    "**/.fseventsd/**",
];

impl Filter {
    pub fn new(patterns: &[String]) -> Result<Self> {
        let mut b = GlobSetBuilder::new();
        for p in DEFAULT_EXCLUDES.iter().copied().chain(patterns.iter().map(String::as_str)) {
            let g = Glob::new(p).map_err(|e| BackupError::BadPattern(format!("{p}: {e}")))?;
            b.add(g);
            // "node_modules" hay "*.tmp" -> cũng khớp ở mọi cấp thư mục
            if !p.contains('/') {
                if let Ok(g) = Glob::new(&format!("**/{p}")) {
                    b.add(g);
                }
                if let Ok(g) = Glob::new(&format!("**/{p}/**")) {
                    b.add(g);
                }
            }
        }
        let set = b.build().map_err(|e| BackupError::BadPattern(e.to_string()))?;
        Ok(Self { set })
    }

    pub fn is_excluded(&self, rel: &Path) -> bool {
        let s = to_slash(rel);
        self.set.is_match(&s)
    }
}

fn to_slash(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Các file nội bộ của app luôn bị bỏ qua khi quét.
fn is_internal(rel: &Path) -> bool {
    let mut comps = rel.components();
    let first = comps.next().map(|c| c.as_os_str().to_string_lossy().into_owned());
    if matches!(first.as_deref(), Some(VERSIONS_DIR) | Some(MARKER_FILE)) {
        return true;
    }
    rel.to_string_lossy().ends_with(TMP_SUFFIX)
}

/// Quét `root`. Lỗi đọc từng file (permission denied...) được ghi vào
/// `warnings` thay vì làm hỏng cả lượt backup. Symlink không được follow.
pub fn scan(root: &Path, filter: &Filter) -> Result<ScanResult> {
    let mut out = ScanResult::default();
    if !root.exists() {
        return Ok(out);
    }

    let mut it = WalkDir::new(root).follow_links(false).into_iter();
    while let Some(entry) = it.next() {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                out.warnings.push(format!("Bỏ qua: {e}"));
                continue;
            }
        };
        let rel = match entry.path().strip_prefix(root) {
            Ok(r) if !r.as_os_str().is_empty() => r.to_path_buf(),
            _ => continue,
        };

        let ft = entry.file_type();
        if is_internal(&rel) || filter.is_excluded(&rel) {
            if ft.is_dir() {
                it.skip_current_dir();
            }
            continue;
        }
        if ft.is_symlink() {
            out.warnings.push(format!("Bỏ qua symlink: {}", rel.display()));
            continue;
        }
        if !ft.is_file() {
            continue;
        }
        match entry.metadata() {
            Ok(md) => {
                out.files.insert(
                    rel,
                    FileEntry {
                        size: md.len(),
                        mtime: md.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                    },
                );
            }
            Err(e) => out.warnings.push(format!("Không đọc được metadata {}: {e}", rel.display())),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn excludes_work() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        fs::create_dir_all(r.join("src/node_modules/x")).unwrap();
        fs::create_dir_all(r.join(".versions/old")).unwrap();
        fs::write(r.join("src/a.txt"), "a").unwrap();
        fs::write(r.join("src/node_modules/x/b.js"), "b").unwrap();
        fs::write(r.join("src/c.tmp"), "c").unwrap();
        fs::write(r.join(".DS_Store"), "").unwrap();
        fs::write(r.join(".versions/old/z"), "").unwrap();
        fs::write(r.join(".backup-id"), "{}").unwrap();

        let f = Filter::new(&["node_modules".into(), "*.tmp".into()]).unwrap();
        let res = scan(r, &f).unwrap();
        let keys: Vec<_> = res.files.keys().map(|p| to_slash(p)).collect();
        assert_eq!(keys, vec!["src/a.txt"]);
    }
}
