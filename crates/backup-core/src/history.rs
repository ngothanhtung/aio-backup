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
        .unwrap_or_else(|| PathBuf::from("usb-backup-history.jsonl"))
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
