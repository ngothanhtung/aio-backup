//! File marker `.backup-id` nhận diện ổ đích bằng UUID, không phụ thuộc
//! ký tự ổ (E:, F:) hay mount path (/media/..., /Volumes/...).

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{BackupError, IoContext, Result};
use crate::MARKER_FILE;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TargetMarker {
    pub id: Uuid,
    pub label: String,
    pub created_at: DateTime<Utc>,
    /// Phiên bản định dạng marker, để nâng cấp sau này.
    #[serde(default = "default_format")]
    pub format: u32,
}

fn default_format() -> u32 {
    1
}

/// Đọc marker ở thư mục gốc. Trả `None` nếu không có hoặc file hỏng.
pub fn read_marker(root: &Path) -> Option<TargetMarker> {
    let raw = std::fs::read_to_string(root.join(MARKER_FILE)).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Đánh dấu một ổ (hoặc thư mục) làm đích backup.
pub fn init_target(root: &Path, label: &str, force: bool) -> Result<TargetMarker> {
    if !force {
        if let Some(existing) = read_marker(root) {
            return Err(BackupError::MarkerExists(existing.id));
        }
    }
    let marker = TargetMarker {
        id: Uuid::new_v4(),
        label: label.to_string(),
        created_at: Utc::now(),
        format: 1,
    };
    let path = root.join(MARKER_FILE);
    let json = serde_json::to_string_pretty(&marker).map_err(anyhow::Error::from)?;
    std::fs::write(&path, json).at(&path)?;
    Ok(marker)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let m = init_target(dir.path(), "USB Samsung T7", false).unwrap();
        assert_eq!(read_marker(dir.path()), Some(m.clone()));
        // Lần 2 không force -> lỗi
        assert!(matches!(
            init_target(dir.path(), "x", false),
            Err(BackupError::MarkerExists(id)) if id == m.id
        ));
        // force -> id mới
        let m2 = init_target(dir.path(), "x", true).unwrap();
        assert_ne!(m2.id, m.id);
    }
}
