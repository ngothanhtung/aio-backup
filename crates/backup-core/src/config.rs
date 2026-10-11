//! Cấu hình job, lưu dạng TOML trong thư mục config chuẩn của từng OS:
//! - Windows: %APPDATA%\Softech\aio-backup\config\config.toml
//! - macOS:   ~/Library/Application Support/vn.Softech.aio-backup/config.toml
//! - Linux:   ~/.config/aio-backup/config.toml

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
fn default_excludes() -> Vec<String> {
    ["node_modules", ".git", "*.tmp"].map(String::from).to_vec()
}
fn default_keep_days() -> u32 {
    30
}

fn default_ftp_port() -> u16 {
    21
}

/// Nơi lưu bản backup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Destination {
    /// Ổ USB/ổ ngoài, nhận diện qua UUID trong file `.backup-id`.
    Usb { target_id: Uuid },
    /// Máy chủ FTP/FTPS (NAS Synology, hosting...).
    Ftp(FtpTarget),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FtpTarget {
    pub host: String,
    #[serde(default = "default_ftp_port")]
    pub port: u16,
    pub username: String,
    /// Thư mục gốc trên server, vd `/backup`. Rỗng = thư mục home của user.
    #[serde(default)]
    pub remote_dir: String,
    /// FTPS explicit (AUTH TLS). Nên bật nếu server hỗ trợ.
    #[serde(default)]
    pub secure: bool,
    /// Chấp nhận chứng chỉ tự ký (NAS trong mạng LAN). Không dùng qua Internet.
    #[serde(default)]
    pub accept_invalid_certs: bool,
    /// Chỉ dùng khi OS không có keychain (Linux headless). Bình thường mật khẩu
    /// nằm trong Credential Manager / Keychain / Secret Service, không ở đây.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

impl FtpTarget {
    pub fn url(&self) -> String {
        let scheme = if self.secure { "ftps" } else { "ftp" };
        let dir = self.remote_dir.trim_matches('/');
        if dir.is_empty() {
            format!("{scheme}://{}@{}:{}", self.username, self.host, self.port)
        } else {
            format!("{scheme}://{}@{}:{}/{dir}", self.username, self.host, self.port)
        }
    }
}

impl Destination {
    pub fn usb_id(&self) -> Option<Uuid> {
        match self {
            Destination::Usb { target_id } => Some(*target_id),
            Destination::Ftp(_) => None,
        }
    }
    pub fn ftp(&self) -> Option<&FtpTarget> {
        match self {
            Destination::Ftp(f) => Some(f),
            Destination::Usb { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Job {
    pub id: Uuid,
    pub name: String,
    /// Thư mục nguồn trên HDD/SSD.
    pub source: PathBuf,
    pub destination: Destination,
    /// Thư mục con ở đích; rỗng = dùng tên job.
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
    /// USB: tự chạy khi phát hiện ổ đích được cắm vào.
    #[serde(default = "yes")]
    pub auto_run: bool,
    /// Chạy định kỳ mỗi N phút (FTP; với USB thì chỉ khi ổ đang cắm).
    #[serde(default)]
    pub interval_minutes: Option<u32>,
}

impl Job {
    pub fn new(name: impl Into<String>, source: impl Into<PathBuf>, destination: Destination) -> Self {
        let is_ftp = matches!(destination, Destination::Ftp(_));
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            source: source.into(),
            destination,
            dest_subdir: String::new(),
            excludes: vec![],
            mirror_deletes: false,
            versioning: true,
            keep_versions_days: Some(30),
            verify_hash: false,
            auto_run: true,
            // FTP không có sự kiện "cắm vào" nên mặc định chạy mỗi giờ
            interval_minutes: is_ftp.then_some(60),
        }
    }

    pub fn usb(name: impl Into<String>, source: impl Into<PathBuf>, target_id: Uuid) -> Self {
        Self::new(name, source, Destination::Usb { target_id })
    }

    /// Mô tả đích để hiển thị.
    pub fn destination_label(&self) -> String {
        match &self.destination {
            Destination::Usb { target_id } => format!("USB {target_id}"),
            Destination::Ftp(f) => f.url(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Config {
    /// Chu kỳ kiểm tra ổ đĩa và lịch (giây).
    #[serde(default = "default_poll")]
    pub poll_interval_secs: u64,
    /// Danh sách loại trừ điền sẵn khi tạo job mới.
    #[serde(default = "default_excludes")]
    pub default_excludes: Vec<String>,
    /// Số ngày giữ bản cũ điền sẵn khi tạo job mới (0 = giữ mãi).
    #[serde(default = "default_keep_days")]
    pub default_keep_days: u32,
    #[serde(default)]
    pub jobs: Vec<Job>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            poll_interval_secs: default_poll(),
            default_excludes: default_excludes(),
            default_keep_days: default_keep_days(),
            jobs: vec![],
        }
    }
}

pub fn project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("vn", "Softech", "aio-backup")
}

pub fn default_config_path() -> PathBuf {
    project_dirs()
        .map(|d| d.config_dir().join("config.toml"))
        .unwrap_or_else(|| PathBuf::from("aio-backup.toml"))
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
        c.jobs.push(Job::usb("Tai lieu", "/home/a/Documents", Uuid::new_v4()));
        c.jobs.push(Job::new(
            "NAS",
            "/home/a/Pictures",
            Destination::Ftp(FtpTarget {
                host: "192.168.1.10".into(),
                port: 21,
                username: "backup".into(),
                remote_dir: "/backup".into(),
                secure: true,
                accept_invalid_certs: true,
                password: None,
            }),
        ));
        c.save(&p).unwrap();
        let back = Config::load(&p).unwrap();
        assert_eq!(back, c);
        assert!(back.find_job("tai LIEU").is_some());
    }
}
