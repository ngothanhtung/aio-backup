//! So sánh nguồn và đích để lập kế hoạch (chưa ghi gì ra đĩa).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Serialize;

use crate::error::Result;
use crate::names::path_problem;
use crate::scan::{scan, FileEntry, Filter};
use crate::MTIME_TOLERANCE_SECS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionKind {
    /// File mới, chưa có ở đích.
    Create,
    /// File đã có nhưng khác (size/mtime/hash).
    Update,
    /// File có ở đích nhưng đã bị xoá ở nguồn (chỉ khi mirror_deletes).
    Delete,
}

#[derive(Debug, Clone, Serialize)]
pub struct Action {
    pub kind: ActionKind,
    pub rel: PathBuf,
    pub size: u64,
}

#[derive(Debug, Default, Serialize)]
pub struct Plan {
    pub actions: Vec<Action>,
    pub bytes_to_copy: u64,
    pub unchanged: usize,
    pub warnings: Vec<String>,
}

impl Plan {
    pub fn count(&self, kind: ActionKind) -> usize {
        self.actions.iter().filter(|a| a.kind == kind).count()
    }
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct PlanOptions {
    pub excludes: Vec<String>,
    /// Xoá ở đích những file đã bị xoá ở nguồn (mirror). Mặc định: không.
    pub mirror_deletes: bool,
    /// Khi size bằng nhau nhưng mtime khác, so blake3 trước khi copy lại.
    /// Hữu ích khi file bị "touch" mà nội dung không đổi.
    pub verify_hash: bool,
}

pub fn build_plan(src: &Path, dst: &Path, opts: &PlanOptions) -> Result<Plan> {
    if !src.is_dir() {
        return Err(crate::BackupError::SourceMissing(src.to_path_buf()));
    }
    let filter = Filter::new(&opts.excludes)?;
    let s = scan(src, &filter)?;
    // Áp cùng bộ lọc cho đích: file bị exclude không bao giờ bị đụng tới,
    // kể cả khi bật mirror_deletes.
    let d = scan(dst, &filter)?;

    let mut plan = Plan {
        warnings: s.warnings,
        ..Default::default()
    };
    plan.warnings.extend(d.warnings);

    for (rel, se) in &s.files {
        if let Some(why) = path_problem(rel) {
            plan.warnings.push(format!("Bỏ qua {}: {why}", rel.display()));
            continue;
        }
        let kind = match d.files.get(rel) {
            None => Some(ActionKind::Create),
            Some(de) if same(se, de) => None,
            Some(de) if opts.verify_hash && se.size == de.size => {
                match (hash(&src.join(rel)), hash(&dst.join(rel))) {
                    (Ok(a), Ok(b)) if a == b => None,
                    _ => Some(ActionKind::Update),
                }
            }
            Some(_) => Some(ActionKind::Update),
        };
        match kind {
            Some(kind) => {
                plan.bytes_to_copy += se.size;
                plan.actions.push(Action {
                    kind,
                    rel: rel.clone(),
                    size: se.size,
                });
            }
            None => plan.unchanged += 1,
        }
    }

    if opts.mirror_deletes {
        for (rel, de) in &d.files {
            if !s.files.contains_key(rel) {
                plan.actions.push(Action {
                    kind: ActionKind::Delete,
                    rel: rel.clone(),
                    size: de.size,
                });
            }
        }
    }
    Ok(plan)
}

fn same(a: &FileEntry, b: &FileEntry) -> bool {
    if a.size != b.size {
        return false;
    }
    let diff = match a.mtime.duration_since(b.mtime) {
        Ok(d) => d,
        Err(e) => e.duration(),
    };
    diff <= Duration::from_secs(MTIME_TOLERANCE_SECS)
}

pub(crate) fn hash(path: &Path) -> std::io::Result<blake3::Hash> {
    let mut f = std::fs::File::open(path)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize())
}
