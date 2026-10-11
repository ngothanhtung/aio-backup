<#
.SYNOPSIS
  Tạo chứng chỉ ký code nội bộ (tự ký) cho AIO Backup.

.DESCRIPTION
  Chạy MỘT LẦN trên máy quản trị (Windows 10/11, PowerShell 5.1+). Tạo ra:
    - aio-codesign.pfx       : khoá bí mật, dùng cho CI ký bản cài đặt. GIỮ KÍN.
    - aio-codesign.cer       : chứng chỉ công khai, cài lên các máy trong công ty.
    - aio-codesign.pfx.b64   : nội dung base64 của .pfx để đưa vào GitHub secret.
  Sau đó in lệnh khai báo secret cho repo.

.EXAMPLE
  .\New-InternalCodeSigningCert.ps1 -Organization "Softech Aptech Da Nang" -OutDir C:\certs
#>
param(
  [string]$Organization = "Softech Aptech Da Nang",
  [string]$OutDir = (Join-Path $PWD "aio-codesign"),
  [int]$Years = 5,
  [string]$Repo = "ngothanhtung/aio-backup"
)

$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

$password = Read-Host "Đặt mật khẩu cho file .pfx (dùng làm secret WINDOWS_CERTIFICATE_PASSWORD)" -AsSecureString

$cert = New-SelfSignedCertificate `
  -Type CodeSigningCert `
  -Subject "CN=$Organization (AIO Backup), O=$Organization" `
  -FriendlyName "AIO Backup Internal Code Signing" `
  -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 `
  -KeyExportPolicy Exportable `
  -CertStoreLocation Cert:\CurrentUser\My `
  -NotAfter (Get-Date).AddYears($Years)

$pfx = Join-Path $OutDir "aio-codesign.pfx"
$cer = Join-Path $OutDir "aio-codesign.cer"
$b64 = Join-Path $OutDir "aio-codesign.pfx.b64"

Export-PfxCertificate -Cert $cert -FilePath $pfx -Password $password | Out-Null
Export-Certificate -Cert $cert -FilePath $cer -Type CERT | Out-Null
[Convert]::ToBase64String([IO.File]::ReadAllBytes($pfx)) | Set-Content -NoNewline -Encoding ascii $b64

# Khoá đã nằm trong file .pfx -> gỡ khỏi kho chứng chỉ của máy này
Remove-Item -Path ("Cert:\CurrentUser\My\" + $cert.Thumbprint)

Write-Host ""
Write-Host "Đã tạo chứng chỉ:" -ForegroundColor Green
Write-Host "  Subject    : $($cert.Subject)"
Write-Host "  Thumbprint : $($cert.Thumbprint)"
Write-Host "  Hết hạn    : $($cert.NotAfter.ToString('dd/MM/yyyy'))"
Write-Host "  Thư mục    : $OutDir"
Write-Host ""
Write-Host "Bước 1 - khai báo secret cho CI (cần GitHub CLI, đã 'gh auth login'):" -ForegroundColor Cyan
Write-Host "  Get-Content `"$b64`" -Raw | gh secret set WINDOWS_CERTIFICATE --repo $Repo"
Write-Host "  gh secret set WINDOWS_CERTIFICATE_PASSWORD --repo $Repo"
Write-Host "  (hoặc dán nội dung file .b64 vào GitHub > Settings > Secrets and variables > Actions)"
Write-Host ""
Write-Host "Bước 2 - cài aio-codesign.cer lên máy trong công ty: GPO hoặc Install-InternalCert.ps1" -ForegroundColor Cyan
Write-Host ""
Write-Host "Sau khi khai báo secret: XOÁ file .pfx.b64, cất file .pfx ở nơi an toàn (két, vault)." -ForegroundColor Yellow
