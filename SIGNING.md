# Ký số bản cài đặt

CI (`.github/workflows/build.yml`) **tự bật ký số khi có secrets** và tự kiểm tra chữ ký sau khi build.
Chưa khai báo secrets thì build vẫn chạy:

| Nền tảng | Không có secrets | Có secrets |
|----------|------------------|------------|
| Windows  | Không ký: SmartScreen báo "Unknown publisher" | Ký Authenticode `.exe`/`.msi` (qua [jsign](https://ebourg.github.io/jsign/)), kèm timestamp |
| macOS    | Ký ad-hoc: chạy được nhưng phải cho phép trong Privacy & Security | Ký Developer ID + notarize, mở bình thường |
| Linux    | Không cần ký | — |

Secrets khai báo ở **GitHub → Settings → Secrets and variables → Actions → New repository secret**,
hoặc bằng `gh secret set TÊN < file`.

---

## Dùng nội bộ (cách đang áp dụng cho AIO Backup)

Không cần mua chứng chỉ. Tự tạo một chứng chỉ ký code cho công ty, cho các máy trong công ty tin nó,
rồi để CI ký mọi bản cài đặt bằng chứng chỉ đó. Máy ngoài công ty vẫn coi là "không rõ nguồn gốc".

### Bước 1: Tạo chứng chỉ (một lần, trên máy Windows của quản trị)

```powershell
cd scripts\internal-cert
.\New-InternalCodeSigningCert.ps1 -Organization "Softech Aptech Da Nang" -OutDir C:\certs\aio
```

Script hỏi mật khẩu cho file `.pfx`, tạo chứng chỉ RSA 3072 hạn 5 năm và in sẵn lệnh khai báo secret.
Kết quả gồm 3 file:

| File | Dùng làm gì | Bảo quản |
|------|-------------|----------|
| `aio-codesign.pfx` | Khoá bí mật để ký | **Giữ kín** (két, password manager) |
| `aio-codesign.pfx.b64` | Nội dung đưa vào secret | Xoá sau khi khai báo secret |
| `aio-codesign.cer` | Chứng chỉ công khai | Phát cho các máy, để trên share IT |

### Bước 2: Khai báo 2 secret cho repo

```powershell
Get-Content C:\certs\aio\aio-codesign.pfx.b64 -Raw | gh secret set WINDOWS_CERTIFICATE --repo ngothanhtung/aio-backup
gh secret set WINDOWS_CERTIFICATE_PASSWORD --repo ngothanhtung/aio-backup   # nhập mật khẩu .pfx
```

Hoặc vào GitHub → Settings → Secrets and variables → Actions → New repository secret.
Lần build sau, bước **Kiểm tra chữ ký Windows** sẽ in `Valid` cho từng file `.exe`/`.msi`.

### Bước 3: Cho các máy trong công ty tin chứng chỉ

**Máy join domain (khuyên dùng):** Group Policy Management → tạo/sửa GPO gắn vào OU chứa máy →
Computer Configuration → Policies → Windows Settings → Security Settings → Public Key Policies:

- *Trusted Root Certification Authorities* → Import → `aio-codesign.cer`
- *Trusted Publishers* → Import → `aio-codesign.cer`

Máy nhận chính sách ở lần khởi động lại hoặc khi chạy `gpupdate /force`.

**Máy lẻ, không join domain:** chạy PowerShell bằng quyền Administrator:

```powershell
.\Install-InternalCert.ps1 -CertPath \\nas\it\aio-backup\aio-codesign.cer
# Gỡ: thêm -Remove
```

### Bước 4: Phân phối bản cài đặt

- **Để file cài trên share nội bộ / NAS và cài từ đó.** File tải bằng trình duyệt bị gắn đánh dấu
  "tải từ Internet", và SmartScreen vẫn có thể hỏi lại vì chứng chỉ nội bộ không có uy tín công khai.
- Cài im lặng hàng loạt (GPO startup script, PDQ, Intune...):
  - NSIS: `"AIO Backup_0.1.0_x64-setup.exe" /S`
  - MSI: `msiexec /i "AIO Backup_0.1.0_x64_en-US.msi" /qn`
- Bản cài có kèm timestamp nên **vẫn hợp lệ sau khi chứng chỉ hết hạn**. Trước khi hết hạn 5 năm,
  tạo chứng chỉ mới, phát `.cer` mới cho các máy trước, rồi mới thay secret.

### macOS và Linux trong công ty

- **macOS:** CI đã ký ad-hoc. Chép `AIO Backup.app` từ share nội bộ vào `/Applications` thường
  mở được ngay. Nếu tải bằng trình duyệt và macOS báo chặn: chuột phải → Open, hoặc
  `xattr -dr com.apple.quarantine "/Applications/AIO Backup.app"`. Chỉ cần Apple Developer Program
  (99 USD/năm) khi phát hành ra ngoài.
- **Linux:** không cần ký; cài bằng `sudo apt install "./AIO Backup_0.1.0_amd64.deb"`.

---

## Windows: các lựa chọn khi phát hành ra ngoài

Từ 6/2023, chứng chỉ OV/EV mới phải nằm trên phần cứng (token/HSM), nên không còn xuất được file
`.pfx` để đưa vào CI. Cách làm được trên CI là **ký qua dịch vụ đám mây** của nhà cung cấp chứng chỉ.

Chứng chỉ mới (OV, kể cả EV) vẫn bị SmartScreen cảnh báo ở vài bản đầu, cho tới khi tích luỹ đủ
"uy tín". Ký đều đặn bằng cùng một chứng chỉ thì cảnh báo sẽ hết.

### Cách 1: SSL.com OV + eSigner (khuyên dùng ở Việt Nam)

1. Mua chứng chỉ **Code Signing (OV)** của SSL.com và đăng ký **eSigner** cho chứng chỉ đó.
2. Trong tài khoản SSL.com: Orders → chứng chỉ → mục *Signing Credentials* lấy **Credential ID**.
3. Bật **eSigner automated signing** để lấy **TOTP secret** dạng base64 (dùng cho máy ký tự động,
   không phải mã 6 số trên điện thoại).
4. Thêm secrets:

| Secret | Giá trị |
|--------|---------|
| `ESIGNER_USERNAME` | Tên đăng nhập SSL.com |
| `ESIGNER_PASSWORD` | Mật khẩu SSL.com |
| `ESIGNER_CREDENTIAL_ID` | Credential ID ở bước 2 |
| `ESIGNER_TOTP_SECRET` | TOTP secret base64 ở bước 3 |

Thử trước với môi trường sandbox của SSL.com bằng cách đặt thêm biến `ESIGNER_SANDBOX=1`.

### Cách 2: SignPath Foundation (miễn phí cho dự án mã nguồn mở)

Repo đang để public, nếu giữ mã nguồn mở thì có thể đăng ký [SignPath Foundation](https://signpath.org).
SignPath ký qua GitHub Action riêng của họ (ký file sau khi build), không dùng script trong repo này;
khi được duyệt sẽ cần thêm một job vào workflow theo hướng dẫn của họ.

### Cách 3: Azure Artifact Signing (trước là Trusted Signing)

Rẻ nhất (khoảng 10 USD/tháng) nhưng hiện chỉ nhận **tổ chức ở Mỹ, Canada, EU, Anh** và **cá nhân ở
Mỹ, Canada**. Nếu sau này có pháp nhân ở các nước đó:

| Secret | Giá trị |
|--------|---------|
| `AZURE_TENANT_ID`, `AZURE_CLIENT_ID`, `AZURE_CLIENT_SECRET` | App registration có quyền *Code Signing Certificate Profile Signer* |
| `AZURE_SIGNING_ENDPOINT` | vd `weu.codesigning.azure.net` |
| `AZURE_SIGNING_ACCOUNT` | Tên tài khoản Artifact Signing |
| `AZURE_SIGNING_PROFILE` | Tên certificate profile |

### Cách 4: File PFX

Dùng cho chứng chỉ nội bộ (xem mục **Dùng nội bộ** ở trên) hoặc chứng chỉ thương mại mua trước 6/2023
còn xuất được `.pfx`.

| Secret | Giá trị |
|--------|---------|
| `WINDOWS_CERTIFICATE` | Nội dung base64 của file `.pfx` |
| `WINDOWS_CERTIFICATE_PASSWORD` | Mật khẩu file `.pfx` |


### Thử script ký ở máy dev

```bash
# Cần Java 11+ và jsign: https://github.com/ebourg/jsign/releases
JSIGN_JAR=./jsign-7.5.jar \
WINDOWS_CERTIFICATE="$(base64 -w0 aio-codesign.pfx)" WINDOWS_CERTIFICATE_PASSWORD='...' \
WINDOWS_TIMESTAMP_URL=none \
node scripts/sign-windows.mjs "path/to/AIO Backup_0.1.0_x64-setup.exe"

osslsigncode verify "path/to/AIO Backup_0.1.0_x64-setup.exe"   # hoặc Get-AuthenticodeSignature trên Windows
```

---

## macOS

Cần tài khoản **Apple Developer Program** trả phí (99 USD/năm). Tài khoản miễn phí không notarize được.

1. Tạo chứng chỉ **Developer ID Application** (chỉ Account Holder tạo được) trên developer.apple.com,
   cài vào Keychain của máy Mac.
2. Trong Keychain Access: chuột phải chứng chỉ → Export → `.p12`, đặt mật khẩu.
3. `base64 -i DeveloperID.p12 | pbcopy` để lấy nội dung base64.
4. Lấy tên identity: `security find-identity -v -p codesigning`, vd
   `Developer ID Application: Ten Cua Ban (ABCDE12345)`.

| Secret | Giá trị |
|--------|---------|
| `APPLE_CERTIFICATE` | Nội dung base64 của `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | Mật khẩu `.p12` |
| `APPLE_SIGNING_IDENTITY` | Tên identity ở bước 4 |

**Notarize** — chọn một trong hai:

*App Store Connect API key (khuyên dùng):* App Store Connect → Users and Access → Integrations → Keys,
tạo key quyền Developer, tải file `.p8` (chỉ tải được một lần).

| Secret | Giá trị |
|--------|---------|
| `APPLE_API_ISSUER` | Issuer ID (phía trên bảng key) |
| `APPLE_API_KEY` | Key ID |
| `APPLE_API_PRIVATE_KEY` | Toàn bộ nội dung file `.p8` |

*Apple ID:* tạo app-specific password ở appleid.apple.com.

| Secret | Giá trị |
|--------|---------|
| `APPLE_ID` | Email Apple ID |
| `APPLE_PASSWORD` | App-specific password |
| `APPLE_TEAM_ID` | Team ID (10 ký tự) |

---

## Kiểm tra sau khi CI chạy

- Log bước **Chuẩn bị ký số** cho biết nền tảng nào được ký.
- Bước **Kiểm tra chữ ký Windows** in trạng thái từng file; có secrets mà file chưa ký hợp lệ thì job báo lỗi.
- Bước **Kiểm tra chữ ký macOS** chạy `codesign --verify` và `spctl` (Gatekeeper).
- Phát hành: tạo tag `v0.1.0` và push, CI tạo bản **Draft release** có sẵn file cài đặt.
