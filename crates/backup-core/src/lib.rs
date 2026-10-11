//! backup-core: logic backup của AIO Backup, không phụ thuộc UI.
//!
//! Luồng chính:
//! 1. [`job::open_storage`] mở đích: ổ USB (tìm theo UUID trong `.backup-id`)
//!    hoặc máy chủ FTP/FTPS. Cả hai cài đặt trait [`storage::Storage`].
//! 2. [`plan::build_plan_for`] so sánh nguồn và đích (size + mtime, tuỳ chọn blake3).
//! 3. [`engine::execute`] thực thi: ghi atomic (tmp → rename), chuyển bản cũ
//!    vào `.versions/<thời điểm>/` nếu bật versioning.
//! 4. [`schedule::Scheduler`] quyết định job nào tới lượt chạy (cắm USB / định kỳ).

pub mod config;
pub mod drives;
pub mod engine;
pub mod error;
pub mod history;
pub mod job;
pub mod marker;
pub mod names;
pub mod plan;
pub mod restore;
pub mod scan;
pub mod schedule;
pub mod secrets;
pub mod storage;

pub use error::{BackupError, Result};

/// File marker đặt ở thư mục gốc ổ đích.
pub const MARKER_FILE: &str = ".backup-id";
/// Manifest trạng thái ở đích FTP.
pub const MANIFEST_FILE: &str = ".aio-manifest.json";
/// Thư mục chứa các phiên bản cũ trong thư mục đích.
pub const VERSIONS_DIR: &str = ".versions";
/// Đuôi file tạm khi copy.
pub const TMP_SUFFIX: &str = ".backup-tmp";
/// Sai số mtime chấp nhận được (FAT32 lưu mtime với độ phân giải 2 giây).
pub const MTIME_TOLERANCE_SECS: u64 = 2;
