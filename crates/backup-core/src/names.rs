//! Kiểm tra tên file có ghi được lên exFAT/NTFS hay không.
//! File tạo trên Linux/macOS có thể chứa `:` `?` `*`... mà exFAT không cho phép.

use std::path::Path;

const FORBIDDEN: &[char] = &['<', '>', ':', '"', '\\', '|', '?', '*'];
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub fn is_forbidden_char(c: char) -> bool {
    FORBIDDEN.contains(&c) || (c as u32) < 0x20
}

/// Lý do một thành phần tên không portable, `None` nếu hợp lệ.
pub fn component_problem(name: &str) -> Option<&'static str> {
    if name.chars().any(is_forbidden_char) {
        return Some("chứa ký tự không hợp lệ trên exFAT/NTFS");
    }
    if name.ends_with(' ') || name.ends_with('.') {
        return Some("kết thúc bằng dấu cách hoặc dấu chấm");
    }
    let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        return Some("trùng tên dành riêng của Windows");
    }
    None
}

/// Kiểm tra toàn bộ đường dẫn tương đối.
pub fn path_problem(rel: &Path) -> Option<String> {
    for comp in rel.components() {
        let s = comp.as_os_str().to_string_lossy();
        if let Some(why) = component_problem(&s) {
            return Some(format!("'{s}' {why}"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_bad_names() {
        assert!(component_problem("report.pdf").is_none());
        assert!(component_problem("Bài giảng 01.docx").is_none());
        assert!(component_problem("a:b.txt").is_some());
        assert!(component_problem("what?").is_some());
        assert!(component_problem("trailing.").is_some());
        assert!(component_problem("con.txt").is_some());
        assert!(path_problem(Path::new("ok/sub/bad|name")).is_some());
    }
}
