use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    #[error("Thư mục nguồn không tồn tại: {0}")]
    SourceMissing(PathBuf),

    #[error("Không tìm thấy ổ đích có id {0} (USB chưa cắm?)")]
    TargetNotFound(uuid::Uuid),

    #[error("Ổ đích đã có marker (id {0}); dùng force để ghi đè")]
    MarkerExists(uuid::Uuid),

    #[error("Không đủ dung lượng: cần {} MB, ổ đích còn {} MB", .needed / 1_048_576, .available / 1_048_576)]
    InsufficientSpace { needed: u64, available: u64 },

    #[error("Đã huỷ")]
    Cancelled,

    #[error("Job không tồn tại: {0}")]
    JobNotFound(String),

    #[error("Pattern exclude không hợp lệ: {0}")]
    BadPattern(String),

    #[error("I/O lỗi tại {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, BackupError>;

/// Gắn đường dẫn vào lỗi I/O để log dễ đọc.
pub(crate) trait IoContext<T> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T>;
}

impl<T> IoContext<T> for std::io::Result<T> {
    fn at(self, path: impl Into<PathBuf>) -> Result<T> {
        self.map_err(|source| BackupError::Io {
            path: path.into(),
            source,
        })
    }
}
