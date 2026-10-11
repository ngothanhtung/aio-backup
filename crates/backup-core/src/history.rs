//! Lịch sử chạy job dạng JSON Lines (mỗi dòng một lượt chạy).
//! Đủ dùng cho MVP; muốn lọc/thống kê nhiều thì chuyển sang SQLite (rusqlite).

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::project_dirs;
use crate::engine::Report;
use crate::error::{IoContext, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub job_id: Uuid,
    pub job_name: String,
    /// Report lưu dạng JSON tự do để không vỡ khi struct Report đổi.
    pub report: serde_json::Value,
}

pub fn default_history_path() -> PathBuf {
    project_dirs()
        .map(|d| d.data_dir().join("history.jsonl"))
        .unwrap_or_else(|| PathBuf::from("aio-backup-history.jsonl"))
}

pub fn append(path: &Path, job_id: Uuid, job_name: &str, report: &Report) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).at(p)?;
    }
    let entry = HistoryEntry {
        job_id,
        job_name: job_name.to_string(),
        report: serde_json::to_value(report).map_err(anyhow::Error::from)?,
    };
    let line = serde_json::to_string(&entry).map_err(anyhow::Error::from)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .at(path)?;
    writeln!(f, "{line}").at(path)
}

/// Ghi một lượt chạy thất bại trước khi kịp bắt đầu (không mở được đích,
/// sai mật khẩu FTP...), để màn hình theo dõi vẫn thấy lỗi.
pub fn append_error(
    path: &Path,
    job_id: Uuid,
    job_name: &str,
    started_at: chrono::DateTime<chrono::Utc>,
    dry_run: bool,
    error: &str,
) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).at(p)?;
    }
    let entry = HistoryEntry {
        job_id,
        job_name: job_name.to_string(),
        report: serde_json::json!({
            "startedAt": started_at,
            "finishedAt": chrono::Utc::now(),
            "dryRun": dry_run,
            "created": 0, "updated": 0, "deleted": 0, "unchanged": 0, "bytesCopied": 0,
            "failed": [], "warnings": [], "cancelled": false, "targetLost": false,
            "error": error,
        }),
    };
    let line = serde_json::to_string(&entry).map_err(anyhow::Error::from)?;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .at(path)?;
    writeln!(f, "{line}").at(path)
}

/// Thời điểm chạy thật (không tính dry-run) gần nhất của từng job.
pub fn last_runs(path: &Path) -> std::collections::HashMap<Uuid, chrono::DateTime<chrono::Utc>> {
    let mut out = std::collections::HashMap::new();
    // recent() trả mới nhất trước -> giữ giá trị đầu tiên gặp
    for e in recent(path, usize::MAX) {
        if e.report["dryRun"].as_bool() == Some(true) {
            continue;
        }
        let at = e.report["startedAt"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|d| d.with_timezone(&chrono::Utc));
        if let Some(at) = at {
            out.entry(e.job_id).or_insert(at);
        }
    }
    out
}

/// `limit` lượt gần nhất, mới nhất trước.
pub fn recent(path: &Path, limit: usize) -> Vec<HistoryEntry> {
    let Ok(f) = std::fs::File::open(path) else {
        return vec![];
    };
    let mut all: Vec<HistoryEntry> = BufReader::new(f)
        .lines()
        .map_while(|l| l.ok())
        .filter_map(|l| serde_json::from_str(&l).ok())
        .collect();
    all.reverse();
    all.truncate(limit);
    all
}
