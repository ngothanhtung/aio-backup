//! Liệt kê ổ đĩa đang mount và tìm ổ đích theo marker.
//!
//! Chiến lược "poll + marker" chạy giống nhau trên Windows/macOS/Linux,
//! không cần hook WM_DEVICECHANGE / DiskArbitration / udev riêng.

use std::path::{Path, PathBuf};

use serde::Serialize;
use sysinfo::Disks;
use uuid::Uuid;

use crate::marker::{read_marker, TargetMarker};

#[derive(Debug, Clone, Serialize)]
pub struct DriveInfo {
    pub name: String,
    pub mount_point: PathBuf,
    pub file_system: String,
    pub total_space: u64,
    pub available_space: u64,
    pub removable: bool,
    pub marker: Option<TargetMarker>,
}

/// Danh sách ổ đang mount, kèm marker nếu có.
pub fn list_drives() -> Vec<DriveInfo> {
    let disks = Disks::new_with_refreshed_list();
    let mut out: Vec<DriveInfo> = disks
        .list()
        .iter()
        .map(|d| DriveInfo {
            name: d.name().to_string_lossy().into_owned(),
            mount_point: d.mount_point().to_path_buf(),
            file_system: d.file_system().to_string_lossy().into_owned(),
            total_space: d.total_space(),
            available_space: d.available_space(),
            removable: d.is_removable(),
            marker: read_marker(d.mount_point()),
        })
        .collect();
    // Một số distro mount cùng thiết bị ở nhiều nơi -> bỏ trùng mount point
    out.sort_by(|a, b| a.mount_point.cmp(&b.mount_point));
    out.dedup_by(|a, b| a.mount_point == b.mount_point);
    out
}

/// Tìm ổ đích theo UUID marker.
pub fn find_target(id: Uuid) -> Option<DriveInfo> {
    list_drives()
        .into_iter()
        .find(|d| d.marker.as_ref().is_some_and(|m| m.id == id))
}

/// Dung lượng trống của phân vùng chứa `path` (mount point dài nhất khớp).
pub fn available_space_for(path: &Path) -> Option<u64> {
    let path = dunce_canonical(path);
    let disks = Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| path.starts_with(d.mount_point()))
        .max_by_key(|d| d.mount_point().as_os_str().len())
        .map(|d| d.available_space())
}

/// canonicalize nhưng bỏ prefix `\\?\` trên Windows để so sánh với mount point.
fn dunce_canonical(path: &Path) -> PathBuf {
    let p = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if let Some(rest) = s.strip_prefix(r"\\?\") {
            if !rest.starts_with("UNC\\") {
                return PathBuf::from(rest);
            }
        }
    }
    p
}
