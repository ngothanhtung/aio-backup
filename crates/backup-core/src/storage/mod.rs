//! Lớp trừu tượng nơi lưu backup. Engine chỉ làm việc với trait [`Storage`],
//! nên thêm đích mới (SFTP, S3, WebDAV...) chỉ cần viết thêm một impl.

use std::path::Path;

use crate::error::Result;
use crate::scan::{Filter, ScanResult};

pub mod ftp;
pub mod local;

pub use ftp::FtpStorage;
pub use local::LocalStorage;

/// Mọi đường dẫn `rel` là tương đối so với thư mục gốc của job ở đích.
pub trait Storage {
    /// Mô tả để hiển thị/log, vd `/media/T7/Tai lieu` hoặc `ftp://nas/backup/Tai lieu`.
    fn describe(&self) -> String;

    /// Chuẩn bị trước khi ghi: tạo thư mục gốc, dọn rác từ lần chạy bị ngắt.
    fn prepare(&mut self) -> Result<()>;

    /// Trạng thái hiện có ở đích để so với nguồn.
    fn index(&mut self, filter: &Filter) -> Result<ScanResult>;

    /// Đường dẫn local của file ở đích, nếu có (để so blake3). FTP trả `None`.
    fn local_path(&self, rel: &Path) -> Option<std::path::PathBuf>;

    /// Dung lượng trống, `None` nếu không xác định được.
    fn available_space(&mut self) -> Option<u64>;

    /// Ghi file `src` vào `rel` một cách atomic. Nếu `keep_old` có giá trị,
    /// bản cũ (nếu có) được chuyển tới đó trước khi bị thay.
    fn put(&mut self, src: &Path, rel: &Path, keep_old: Option<&Path>) -> Result<()>;

    /// Xoá `rel`, hoặc chuyển nó tới `keep` nếu có (versioning).
    fn remove(&mut self, rel: &Path, keep: Option<&Path>) -> Result<()>;

    /// Đích còn kết nối được không (USB còn cắm, FTP còn phản hồi).
    fn is_alive(&mut self) -> bool;

    /// Gọi một lần ở cuối lượt chạy (kể cả khi huỷ) để ghi lại trạng thái.
    fn finish(&mut self) -> Result<()>;

    /// Xoá snapshot `.versions` cũ hơn `keep_days` ngày, trả số snapshot đã xoá.
    fn prune_versions(&mut self, keep_days: u32) -> Result<usize> {
        let mut removed = 0;
        for name in self.list_snapshots()? {
            if snapshot_older_than(&name, keep_days) {
                self.remove_snapshot(&name)?;
                removed += 1;
            }
        }
        Ok(removed)
    }

    // ----- Khôi phục -----

    /// Tên các snapshot trong `.versions` (chỉ tên đúng định dạng thời gian).
    fn list_snapshots(&mut self) -> Result<Vec<String>>;

    /// Xoá hẳn một snapshot.
    fn remove_snapshot(&mut self, name: &str) -> Result<()>;

    /// Mọi file (đệ quy) dưới thư mục `rel_dir`; đường dẫn trả về tương đối so với `rel_dir`.
    fn list_files_under(&mut self, rel_dir: &Path) -> Result<Vec<RemoteFile>>;

    /// Tải file `rel` ở đích về đường dẫn local `dest` (ghi đè nếu có).
    fn get(&mut self, rel: &Path, dest: &Path) -> Result<()>;
}

/// Một file ở đích, dùng khi duyệt để khôi phục.
#[derive(Debug, Clone)]
pub struct RemoteFile {
    pub rel: std::path::PathBuf,
    pub size: u64,
    pub mtime: Option<std::time::SystemTime>,
}

/// Tên thư mục có phải snapshot hợp lệ không.
pub(crate) fn is_snapshot_name(name: &str) -> bool {
    chrono::NaiveDateTime::parse_from_str(name, crate::engine::VERSION_FMT).is_ok()
}

/// Đường dẫn tương đối dạng `a/b/c` (dùng cho FTP và manifest).
pub(crate) fn to_slash(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Tên snapshot `.versions/<tên>` -> thời điểm tạo, theo giờ local.
pub(crate) fn snapshot_older_than(name: &str, keep_days: u32) -> bool {
    use chrono::{Local, NaiveDateTime, TimeZone};
    let Ok(naive) = NaiveDateTime::parse_from_str(name, crate::engine::VERSION_FMT) else {
        return false;
    };
    let Some(ts) = Local.from_local_datetime(&naive).single() else {
        return false;
    };
    ts < Local::now() - chrono::Duration::days(keep_days as i64)
}
