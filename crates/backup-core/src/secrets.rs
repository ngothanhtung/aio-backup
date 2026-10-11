//! Mật khẩu FTP nằm trong kho bảo mật của OS:
//! Windows Credential Manager, macOS Keychain, Linux Secret Service (GNOME Keyring/KWallet).
//!
//! Máy không có kho bảo mật (Linux server headless) thì mật khẩu đành lưu trong
//! config, kèm cảnh báo. Biến môi trường `AIO_BACKUP_FTP_PASSWORD` dùng cho cron/CI.

use uuid::Uuid;

use crate::config::{Destination, Job};

const SERVICE: &str = "aio-backup";
pub const PASSWORD_ENV: &str = "AIO_BACKUP_FTP_PASSWORD";

fn entry(job_id: Uuid) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(SERVICE, &format!("ftp:{job_id}"))
}

/// Lưu vào keychain và đọc lại để chắc chắn lưu được.
fn store_in_keychain(job_id: Uuid, password: &str) -> Result<(), String> {
    let e = entry(job_id).map_err(|e| e.to_string())?;
    e.set_password(password).map_err(|e| e.to_string())?;
    match e.get_password() {
        Ok(p) if p == password => Ok(()),
        Ok(_) => Err("keychain trả về giá trị khác".into()),
        Err(e) => Err(e.to_string()),
    }
}

/// Gắn mật khẩu cho job FTP. Trả `Some(cảnh báo)` nếu phải lưu tạm trong config.
pub fn set_job_password(job: &mut Job, password: &str) -> Option<String> {
    let job_id = job.id;
    let Destination::Ftp(ftp) = &mut job.destination else {
        return None;
    };
    match store_in_keychain(job_id, password) {
        Ok(()) => {
            ftp.password = None;
            None
        }
        Err(e) => {
            ftp.password = Some(password.to_string());
            Some(format!(
                "Máy không có kho mật khẩu ({e}); mật khẩu được lưu dạng văn bản trong file config"
            ))
        }
    }
}

/// Thứ tự: keychain → config → biến môi trường.
pub fn load_password(job: &Job) -> Option<String> {
    let ftp = job.destination.ftp()?;
    if let Ok(p) = entry(job.id).and_then(|e| e.get_password()) {
        return Some(p);
    }
    if let Some(p) = &ftp.password {
        return Some(p.clone());
    }
    std::env::var(PASSWORD_ENV).ok().filter(|p| !p.is_empty())
}

/// Xoá mật khẩu khỏi keychain khi xoá job.
pub fn delete_password(job_id: Uuid) {
    if let Ok(e) = entry(job_id) {
        let _ = e.delete_credential();
    }
}
