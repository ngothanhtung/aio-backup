//! backup-core: logic backup thư mục sang ổ USB, không phụ thuộc UI.
//!
//! Luồng chính:
//! 1. [`drives::find_target`] tìm ổ USB theo UUID trong file marker `.backup-id`.
//! 2. [`plan::build_plan`] so sánh nguồn và đích (size + mtime, tuỳ chọn blake3).
//! 3. [`engine::execute`] thực thi: copy atomic (tmp → rename), giữ mtime,
//!    chuyển bản cũ vào `.versions/<thời điểm>/` nếu bật versioning.

pub mod config;
pub mod drives;
pub mod engine;
pub mod error;
pub mod history;
pub mod job;
pub mod marker;
pub mod names;
pub mod plan;
pub mod scan;

pub use error::{BackupError, Result};

/// File marker đặt ở thư mục gốc ổ đích.
pub const MARKER_FILE: &str = ".backup-id";
/// Thư mục chứa các phiên bản cũ trong thư mục đích.
pub const VERSIONS_DIR: &str = ".versions";
/// Đuôi file tạm khi copy.
pub const TMP_SUFFIX: &str = ".backup-tmp";
/// Sai số mtime chấp nhận được (FAT32 lưu mtime với độ phân giải 2 giây).
pub const MTIME_TOLERANCE_SECS: u64 = 2;
