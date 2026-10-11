# USB Backup

App backup thư mục từ HDD/SSD sang ổ USB, chạy trên Windows, macOS và Ubuntu.
Viết bằng Rust, giao diện Tauri 2 + React.

## Cấu trúc

```
usb-backup/
├── crates/
│   ├── backup-core/      # Logic thuần, không phụ thuộc UI, có test
│   │   └── src/
│   │       ├── marker.rs   # File .backup-id: nhận diện ổ đích bằng UUID
│   │       ├── drives.rs   # Liệt kê ổ đĩa (sysinfo), tìm ổ theo marker
│   │       ├── scan.rs     # Duyệt thư mục + exclude glob
│   │       ├── plan.rs     # So sánh nguồn/đích: size + mtime (± 2s), tuỳ chọn blake3
│   │       ├── engine.rs   # Copy atomic, giữ mtime, versioning, huỷ, kiểm tra dung lượng
│   │       ├── names.rs    # Phát hiện tên file không ghi được lên exFAT/NTFS
│   │       ├── config.rs   # Job + config TOML theo chuẩn thư mục từng OS
│   │       ├── history.rs  # Lịch sử chạy (JSON Lines)
│   │       └── job.rs      # Ghép tất cả thành một lượt chạy job
│   └── backup-cli/       # CLI `usb-backup` (test nhanh, chạy headless/cron)
├── apps/desktop/         # App Tauri 2 + React + Vite
│   ├── src/              # UI React
│   └── src-tauri/        # Command, watcher cắm USB, tray icon, autostart
└── .github/workflows/    # CI build installer cho 3 OS
```

## Cách hoạt động

1. **Đánh dấu ổ đích**: ghi file `.backup-id` (chứa UUID) vào gốc ổ USB.
   App nhận ổ bằng UUID nên không phụ thuộc ký tự ổ (`E:`, `F:`) hay mount path
   (`/media/...`, `/Volumes/...`).
2. **Job** = thư mục nguồn + UUID ổ đích + tuỳ chọn. Dữ liệu nằm ở `<ổ USB>/<tên job>/`.
3. **Watcher** poll danh sách ổ mỗi 5 giây; thấy ổ đích vừa được cắm thì tự chạy các job
   bật `auto_run`, xong thì hiện thông báo hệ thống.
4. **Copy an toàn**: ghi vào `file.backup-tmp`, fsync, đặt mtime, rồi mới rename.
   Rút USB giữa chừng thì job dừng lại, lần sau file tạm sót lại được dọn tự động.
5. **Versioning**: file bị ghi đè hoặc xoá được chuyển vào
   `.versions/YYYY-MM-DD_HHMMSS/`, tự dọn sau N ngày (mặc định 30).
6. **Mirror** (tắt mặc định): xoá ở USB những file đã xoá ở nguồn.

## Chạy thử

### Yêu cầu

- Rust stable mới nhất, Node 20+
- Ubuntu: `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libxdo-dev`
- Windows: WebView2 (có sẵn trên Win 10/11), Visual Studio Build Tools
- macOS: Xcode Command Line Tools

### Test core

```bash
cargo test -p backup-core
```

### CLI

```bash
cargo build -p backup-cli
alias ub=./target/debug/usb-backup

ub drives                                   # xem các ổ đang cắm
ub init-target /media/$USER/T7 --label "Samsung T7"   # Windows: ub init-target E:\
ub job add --name "Tai lieu" --source ~/Documents --target /media/$USER/T7 \
           --exclude node_modules --exclude "*.tmp"
ub run "tai lieu" --dry-run                 # xem trước
ub run "tai lieu"
ub watch                                    # chạy nền, cắm USB là tự backup
ub history
```

### App desktop

```bash
cd apps/desktop
npm install
npm run tauri dev       # chạy dev
npm run tauri build     # tạo installer: .msi/.exe, .dmg, .deb/.AppImage
```

Đóng cửa sổ thì app ẩn xuống khay hệ thống, watcher vẫn chạy. Bật "Khởi động cùng máy"
để app tự chạy ẩn khi đăng nhập.

## Config & dữ liệu

| OS      | Config                                                        |
|---------|---------------------------------------------------------------|
| Windows | `%APPDATA%\Softech\usb-backup\config\config.toml`             |
| macOS   | `~/Library/Application Support/vn.Softech.usb-backup/config.toml` |
| Linux   | `~/.config/usb-backup/config.toml`                            |

Lịch sử chạy nằm ở `history.jsonl` trong thư mục data tương ứng.

## Lưu ý theo nền tảng

- **Định dạng USB**: dùng **exFAT** nếu cần cắm cả 3 OS. Tên file chứa `: ? * | " < >`
  (hợp lệ trên Linux/macOS) sẽ bị bỏ qua kèm cảnh báo thay vì làm hỏng lượt chạy.
- **macOS**: cần cấp *Full Disk Access* nếu backup Desktop/Documents/Downloads
  (System Settings → Privacy & Security).
- **Windows**: file đang bị app khác khoá (Outlook .pst, DB đang mở) sẽ báo lỗi riêng
  file đó; muốn copy được cần VSS (xem lộ trình).
- **Linux**: tray cần `libayatana-appindicator3`; GNOME cần extension AppIndicator.

## Lộ trình gợi ý

- [ ] Lịch chạy theo giờ (`tokio-cron-scheduler`) bên cạnh chạy khi cắm USB
- [ ] Chọn nhiều ổ đích cho một job (xoay vòng 2 USB)
- [ ] Khôi phục file từ `.versions` ngay trong UI
- [ ] Copy song song nhiều file nhỏ (`rayon`), progress theo byte cho file lớn
- [ ] Lịch sử chuyển sang SQLite (`rusqlite`) để lọc/thống kê
- [ ] Windows VSS cho file đang bị khoá; nén/mã hoá/dedup qua `rustic_core`
- [ ] Ký code + notarize macOS, auto-update (`tauri-plugin-updater`)
