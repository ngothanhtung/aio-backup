<#
.SYNOPSIS
  Cài chứng chỉ ký code nội bộ của AIO Backup lên một máy Windows (cần quyền Administrator).

.DESCRIPTION
  Dùng cho máy không join domain (máy join domain thì nên phân phối qua Group Policy, xem SIGNING.md).
  Chứng chỉ được đưa vào 2 kho của máy (LocalMachine):
    - Trusted Root Certification Authorities: để Windows coi chữ ký là hợp lệ
    - Trusted Publishers: để không hỏi lại "có tin nhà phát hành này không"

.EXAMPLE
  # Chạy PowerShell "Run as administrator"
  .\Install-InternalCert.ps1 -CertPath \\nas\it\aio-backup\aio-codesign.cer

.EXAMPLE
  .\Install-InternalCert.ps1 -CertPath .\aio-codesign.cer -Remove   # gỡ chứng chỉ
#>
param(
  [Parameter(Mandatory = $true)][string]$CertPath,
  [switch]$Remove
)

$ErrorActionPreference = "Stop"

$isAdmin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
  [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $isAdmin) { throw "Cần chạy PowerShell bằng quyền Administrator." }

$cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2 (Resolve-Path $CertPath).Path
$isCodeSigning = $cert.Extensions |
  Where-Object { $_ -is [System.Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension] } |
  ForEach-Object { $_.EnhancedKeyUsages } |
  Where-Object { $_.Value -eq "1.3.6.1.5.5.7.3.3" }
if (-not $isCodeSigning) { throw "File này không phải chứng chỉ ký code." }

Write-Host "Chứng chỉ: $($cert.Subject)"
Write-Host "Thumbprint: $($cert.Thumbprint)  ·  Hết hạn: $($cert.NotAfter.ToString('dd/MM/yyyy'))"

foreach ($store in "Root", "TrustedPublisher") {
  $path = "Cert:\LocalMachine\$store"
  $existing = Get-ChildItem $path | Where-Object Thumbprint -eq $cert.Thumbprint
  if ($Remove) {
    if ($existing) { $existing | Remove-Item; Write-Host "  Đã gỡ khỏi $store" }
  } elseif ($existing) {
    Write-Host "  Đã có trong $store"
  } else {
    Import-Certificate -FilePath $CertPath -CertStoreLocation $path | Out-Null
    Write-Host "  Đã cài vào $store" -ForegroundColor Green
  }
}
