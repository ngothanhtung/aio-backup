//! Cấu hình job, lưu dạng TOML trong thư mục config chuẩn của từng OS:
//! - Windows: %APPDATA%\Softech\usb-backup\config\config.toml
//! - macOS:   ~/Library/Application Support/vn.Softech.usb-backup/config.toml
//! - Linux:   ~/.config/usb-backup/config.toml

use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{BackupError, IoContext, Result};

fn yes() -> bool {
    true
}
fn default_poll() -> u64 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Job {
    pub id: Uuid,
    pub name: String,
    /// Thư mục nguồn trên HDD/SSD.
    pub source: PathBuf,
    /// UUID trong file `.backup-id` của ổ USB đích.
    pub target_id: Uuid,
    /// Thư mục con trên ổ đích; rỗng = dùng tên job.
    #[serde(default)]
    pub dest_subdir: String,
    #[serde(default)]
    pub excludes: Vec<String>,
    #[serde(default)]
    pub mirror_deletes: bool,
    #[serde(default = "yes")]
    pub versioning: bool,
    /// Tự xoá snapshot trong `.versions` cũ hơn N ngày.
    #[serde(default)]
    pub keep_versions_days: Option<u32>,
    #[serde(default)]
    pub verify_hash: bool,
    /// Tự chạy khi phát hiện ổ đích được cắm vào.
    #[serde(default = "yes")]
    pub auto_run: bool,
}

impl Job {
    pub fn new(name: impl Into<String>, source: impl Into<PathBuf>, target_id: Uuid) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            source: source.into(),
            target_id,
            dest_subdir: String::new(),
            excludes: vec![],
            mirror_deletes: false,
            versioning: true,
            keep_versions_days: Some(30),
            verify_hash: false,
            auto_run: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    #[serde(default = "default_poll")]
    pub poll_interval_secs: u64,
    #[serde(default)]
    pub jobs: Vec<Job>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval_secs: default_poll(),
            jobs: vec![],
        }
    }
}

pub fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("vn", "Softech", "usb-backup")
}

pub fn default_config_path() -> PathBuf {
    project_dirs()
        .map(|d| d.config_dir().join("config.toml"))
        .unwrap_or_else(|| PathBuf::from("usb-backup.toml"))
}

impl Config {
    /// Đọc config; chưa có file thì trả cấu hình mặc định.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(s) => toml::from_str(&s)
                .map_err(|e| BackupError::Other(anyhow::anyhow!("config lỗi ({}): {e}", path.display()))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(BackupError::Io {
                path: path.to_path_buf(),
                source: e,
            }),
        }
    }

    /// Ghi atomic: file tạm rồi rename, tránh hỏng config khi crash.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).at(p)?;
        }
        let s = toml::to_string_pretty(self).map_err(anyhow::Error::from)?;
        let tmp = path.with_extension("toml.tmp");
        std::fs::write(&tmp, s).at(&tmp)?;
        std::fs::rename(&tmp, path).at(path)
    }

    /// Tìm job theo tên (không phân biệt hoa thường) hoặc UUID.
    pub fn find_job(&self, key: &str) -> Option<&Job> {
        self.jobs
            .iter()
            .find(|j| j.id.to_string() == key || j.name.eq_ignore_ascii_case(key))
    }

    pub fn remove_job(&mut self, key: &str) -> Result<Job> {
        let idx = self
            .jobs
            .iter()
            .position(|j| j.id.to_string() == key || j.name.eq_ignore_ascii_case(key))
            .ok_or_else(|| BackupError::JobNotFound(key.to_string()))?;
        Ok(self.jobs.remove(idx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("sub/config.toml");
        assert_eq!(Config::load(&p).unwrap(), Config::default());

        let mut c = Config::default();
        c.jobs.push(Job::new("Tai lieu", "/home/a/Documents", Uuid::new_v4()));
        c.save(&p).unwrap();
        let back = Config::load(&p).unwrap();
        assert_eq!(back, c);
        assert!(back.find_job("tai LIEU").is_some());
    }
}
