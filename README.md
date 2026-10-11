# AIO Backup

App backup thư mục từ HDD/SSD sang **ổ USB** hoặc **máy chủ FTP/FTPS** (NAS Synology,
hosting...), chạy trên Windows, macOS và Ubuntu. Viết bằng Rust, giao diện Tauri 2 + React.

## Cấu trúc

```
aio-backup/
├── crates/
│   ├── backup-core/        # Logic thuần, không phụ thuộc UI, có test
│   │   └── src/
│   │       ├── storage/
│   │       │   ├── mod.rs    # Trait Storage: engine chỉ làm việc qua trait này
│   │       │   ├── local.rs  # Đích là thư mục local (ổ USB)
│   │       │   └── ftp.rs    # Đích FTP/FTPS (suppaftp + rustls), manifest trạng thái
│   │       ├── marker.rs     # File .backup-id: nhận diện ổ USB bằng UUID
│   │       ├── drives.rs     # Liệt kê ổ đĩa (sysinfo), tìm ổ theo marker
│   │       ├── scan.rs       # Duyệt thư mục + exclude glob
│   │       ├── plan.rs       # So sánh nguồn/đích: size + mtime (± 2s), tuỳ chọn blake3
│   │       ├── engine.rs     # Thực thi plan: versioning, huỷ, kiểm tra dung lượng
│   │       ├── schedule.rs   # Job nào tới lượt: cắm USB / định kỳ N phút
│   │       ├── secrets.rs    # Mật khẩu FTP trong keychain của OS
│   │       ├── names.rs      # Phát hiện tên file không ghi được lên exFAT/NTFS
│   │       ├── config.rs     # Job + đích (USB | FTP), config TOML
│   │       ├── restore.rs    # Khôi phục từ bản hiện tại hoặc snapshot .versions
│   │       ├── history.rs    # Lịch sử chạy (JSON Lines), kể cả lượt lỗi
│   │       └── job.rs        # Mở đích + lập plan + chạy
│   └── backup-cli/         # CLI `aio-backup` (test nhanh, chạy headless/cron)
├── apps/desktop/           # App Tauri 2 + React + Vite + Ant Design 6
│   ├── src/
│   │   ├── pages/          # Tổng quan, Job, Lịch sử, Khôi phục, Ổ đĩa & Cài đặt
│   │   ├── components/     # Drawer thêm/sửa job, thẻ trạng thái dùng chung
│   │   └── useAppData.ts   # State + lắng nghe event từ backend
│   └── src-tauri/          # Command, watcher USB + lịch, tray icon, autostart
├── scripts/
│   ├── ftp_test_server.py  # FTP/FTPS server tối giản để test
│   ├── sign-windows.mjs    # Ký Authenticode qua jsign (eSigner / Azure / PFX)
│   └── internal-cert/      # Tạo + cài chứng chỉ ký code nội bộ (PowerShell)
└── .github/workflows/      # CI: test (gồm FTP/FTPS thật) + build installer 3 OS
```

## Cách hoạt động

**Chung cho mọi đích**
- **Job** = thư mục nguồn + đích + tuỳ chọn. Dữ liệu nằm trong `<đích>/<tên job>/`.
- **Ghi an toàn**: ghi vào `file.backup-tmp` rồi mới đổi tên sang tên thật. Mất kết nối
  giữa chừng (rút USB, rớt mạng) thì job dừng lại, lần chạy sau tiếp tục phần còn thiếu.
- **Versioning**: file bị ghi đè hoặc xoá được chuyển vào `.versions/YYYY-MM-DD_HHMMSS/`,
  tự dọn sau N ngày (mặc định 30).
- **Mirror** (tắt mặc định): xoá ở đích những file đã xoá ở nguồn.
- **Lịch**: `interval_minutes` chạy job định kỳ. App và lệnh `watch` kiểm tra mỗi 5 giây.

**Đích USB**
- Đánh dấu ổ bằng file `.backup-id` (chứa UUID) ở gốc ổ, nên không phụ thuộc ký tự ổ
  (`E:`, `F:`) hay mount path (`/media/...`, `/Volumes/...`).
- Cắm ổ đích vào là các job bật `auto_run` tự chạy, xong thì hiện thông báo hệ thống.
- Trước khi chạy có kiểm tra dung lượng trống; mtime file được giữ nguyên.

**Đích FTP/FTPS**
- **FTPS explicit** (AUTH TLS) qua rustls, khuyên dùng. Với NAS dùng chứng chỉ tự ký trong
  LAN, bật "Chấp nhận chứng chỉ tự ký" (`--insecure` trên CLI).
- Trạng thái ở server lưu trong `.aio-manifest.json` (size + mtime của file nguồn lúc upload),
  nên không phải LIST cả cây thư mục và không phụ thuộc server có hỗ trợ đặt mtime hay không.
  Manifest được ghi lại mỗi 50 file / 30 giây trong lúc chạy.
- Mật khẩu nằm trong **Windows Credential Manager / macOS Keychain / Linux Secret Service**,
  không nằm trong file config. Máy không có kho mật khẩu (Linux server headless) thì mật khẩu
  đành lưu trong config kèm cảnh báo, hoặc dùng biến môi trường `AIO_BACKUP_FTP_PASSWORD`.
- Passive mode, có xử lý server sau NAT trả IP nội bộ trong PASV.
- Job FTP mặc định chạy mỗi 60 phút.

> Lưu ý: nếu xoá/sửa tay file trên server, manifest không biết. Muốn tải lại toàn bộ
> thì xoá file `.aio-manifest.json` trong thư mục job trên server.

## Giao diện

| Trang | Nội dung |
|-------|----------|
| **Tổng quan** | Số job, đang chạy, lần backup thành công gần nhất, dung lượng 7 ngày; cảnh báo job có lượt chạy gần nhất bị lỗi; bảng tình trạng từng job kèm lần chạy kế tiếp |
| **Job backup** | Danh sách job, chạy / xem trước / huỷ, tiến độ realtime; thêm/sửa job trong Drawer (USB hoặc FTP, kiểm tra kết nối) |
| **Lịch sử** | Lọc theo job, kết quả, khoảng ngày; bấm một lượt để xem chi tiết file lỗi và cảnh báo |
| **Khôi phục** | Chọn job → bản hiện tại hoặc bản cũ trong `.versions` → chọn file trên cây thư mục → khôi phục về thư mục bất kỳ (giữ cấu trúc, tuỳ chọn ghi đè). Chạy được với cả USB và FTP |
| **Ổ đĩa & Cài đặt** | Ổ đang cắm, đánh dấu ổ đích; khởi động cùng máy, chu kỳ kiểm tra, loại trừ và số ngày giữ bản cũ mặc định |

Giao diện theo chế độ sáng/tối của hệ điều hành, dùng locale tiếng Việt của antd.

## Chạy thử

### Yêu cầu

- Rust stable mới nhất, Node 20+
- Ubuntu: `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev libxdo-dev libdbus-1-dev`
- Windows: WebView2 (có sẵn trên Win 10/11), Visual Studio Build Tools
- macOS: Xcode Command Line Tools

### Test

```bash
cargo test -p backup-core -p backup-cli

# Test tích hợp FTP/FTPS với server thật
pip install pyftpdlib pyopenssl
python3 scripts/ftp_test_server.py --root /tmp/ftp-root &            # FTP, cổng 2121
AIO_TEST_FTP=127.0.0.1:2121:backup:backup123 AIO_TEST_FTP_ROOT=/tmp/ftp-root \
  cargo test -p backup-core --test ftp

python3 scripts/ftp_test_server.py --root /tmp/ftps-root --port 2122 --tls &   # FTPS
AIO_TEST_FTPS=1 AIO_TEST_FTP=127.0.0.1:2122:backup:backup123 AIO_TEST_FTP_ROOT=/tmp/ftps-root \
  cargo test -p backup-core --test ftp
```

### CLI

```bash
cargo build -p backup-cli
alias ab=./target/debug/aio-backup

# USB
ab drives                                             # xem các ổ đang cắm
ab init-target /media/$USER/T7 --label "Samsung T7"   # Windows: ab init-target E:\
ab job add --name "Tai lieu" --source ~/Documents --target /media/$USER/T7 \
           --exclude node_modules --exclude "*.tmp"

# FTP/FTPS (mật khẩu được hỏi khi chạy lệnh)
ab job add --name "NAS" --source ~/Pictures \
           --ftp ftps://backup@192.168.1.10/volume1/backup --insecure --every 120
ab job test nas                 # kiểm tra kết nối + quyền ghi
ab job set-password nas         # đổi mật khẩu

ab run "tai lieu" --dry-run     # xem trước
ab run nas
ab run-all                      # USB đang cắm + mọi job FTP
ab watch                        # chạy nền: cắm USB / tới lịch là tự backup
ab history
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

### Ký số & phát hành

CI tự ký số bản cài đặt Windows/macOS khi đã khai báo secrets. Dùng nội bộ thì tự tạo chứng chỉ
bằng `scripts/internal-cert/New-InternalCodeSigningCert.ps1`, xem [SIGNING.md](SIGNING.md).
Push tag `v*` để CI tạo bản Draft release.

## Config & dữ liệu

| OS      | Config                                                            |
|---------|-------------------------------------------------------------------|
| Windows | `%APPDATA%\Softech\aio-backup\config\config.toml`                 |
| macOS   | `~/Library/Application Support/vn.Softech.aio-backup/config.toml` |
| Linux   | `~/.config/aio-backup/config.toml`                                |

Lịch sử chạy nằm ở `history.jsonl` trong thư mục data tương ứng.

## Lưu ý theo nền tảng

- **Định dạng USB**: dùng **exFAT** nếu cần cắm cả 3 OS. Tên file chứa `: ? * | " < >`
  (hợp lệ trên Linux/macOS) sẽ bị bỏ qua kèm cảnh báo thay vì làm hỏng lượt chạy.
- **Synology**: bật FTP trong Control Panel → File Services → FTP, tick
  "Enable FTP SSL/TLS encryption service (FTPS)". Nên tạo user riêng chỉ có quyền ghi
  vào shared folder backup.
- **macOS**: cần cấp *Full Disk Access* nếu backup Desktop/Documents/Downloads
  (System Settings → Privacy & Security).
- **Windows**: file đang bị app khác khoá (Outlook .pst, DB đang mở) sẽ báo lỗi riêng
  file đó; muốn copy được cần VSS (xem lộ trình).
- **Linux**: tray cần `libayatana-appindicator3`; GNOME cần extension AppIndicator.

## Lộ trình gợi ý

- [ ] Thêm đích SFTP / S3 / WebDAV (chỉ cần cài đặt trait `Storage`)
- [ ] Lệnh "verify" đối chiếu manifest FTP với file thật trên server
- [ ] Chọn nhiều đích cho một job (USB + NAS cùng lúc)
- [ ] Upload song song nhiều file nhỏ, progress theo byte cho file lớn
- [ ] Lịch sử chuyển sang SQLite (`rusqlite`) để lọc/thống kê
- [ ] Windows VSS cho file đang bị khoá; nén/mã hoá/dedup qua `rustic_core`
- [ ] Auto-update (`tauri-plugin-updater`)
