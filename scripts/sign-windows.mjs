#!/usr/bin/env node
// Ký Authenticode cho file .exe/.msi trên Windows bằng jsign (https://ebourg.github.io/jsign/).
// Tauri gọi script này cho từng file qua bundle.windows.signCommand:
//     node scripts/sign-windows.mjs %1
//
// Chọn nhà cung cấp theo biến môi trường có mặt (ưu tiên theo thứ tự dưới đây):
//
//   1. SSL.com eSigner (chứng chỉ OV/EV trên HSM đám mây, mua được ở Việt Nam)
//        ESIGNER_USERNAME, ESIGNER_PASSWORD, ESIGNER_CREDENTIAL_ID, ESIGNER_TOTP_SECRET
//        ESIGNER_SANDBOX=1 để thử với môi trường test của SSL.com
//   2. Azure Artifact Signing (trước là Trusted Signing; chỉ một số quốc gia)
//        AZURE_TENANT_ID, AZURE_CLIENT_ID, AZURE_CLIENT_SECRET,
//        AZURE_SIGNING_ENDPOINT (vd weu.codesigning.azure.net), AZURE_SIGNING_ACCOUNT, AZURE_SIGNING_PROFILE
//   3. File PFX (chứng chỉ cũ trước 6/2023, hoặc chứng chỉ nội bộ cho máy trong công ty)
//        WINDOWS_CERTIFICATE (PFX dạng base64), WINDOWS_CERTIFICATE_PASSWORD
//
// Chung:
//   JSIGN_JAR             đường dẫn jsign-<ver>.jar (CI tự tải)
//   WINDOWS_TIMESTAMP_URL máy chủ timestamp; "none" để tắt (chỉ dùng khi test)
//   SIGN_DESCRIPTION      tên hiển thị trong hộp thoại UAC (mặc định "AIO Backup")

import { spawnSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const file = process.argv[2];
if (!file) {
  console.error("Cách dùng: node sign-windows.mjs <file.exe|file.msi>");
  process.exit(2);
}

const env = process.env;
const jar = env.JSIGN_JAR;
if (!jar) {
  console.error("[sign] Thiếu JSIGN_JAR (đường dẫn tới jsign.jar)");
  process.exit(2);
}

const has = (...names) => names.every((n) => (env[n] ?? "").trim() !== "");
const description = env.SIGN_DESCRIPTION || "AIO Backup";
const common = ["--name", description, "--replace"];
let tempDir = null;
let provider;
let args;

if (has("ESIGNER_USERNAME", "ESIGNER_PASSWORD", "ESIGNER_CREDENTIAL_ID", "ESIGNER_TOTP_SECRET")) {
  provider = "SSL.com eSigner";
  args = [
    "--storetype", "ESIGNER",
    "--storepass", `${env.ESIGNER_USERNAME}|${env.ESIGNER_PASSWORD}`,
    "--alias", env.ESIGNER_CREDENTIAL_ID,
    "--keypass", env.ESIGNER_TOTP_SECRET,
    ...(env.ESIGNER_SANDBOX === "1" ? ["--keystore", "https://cs-try.ssl.com"] : []),
  ];
  timestamp(args, "http://ts.ssl.com");
} else if (
  has("AZURE_TENANT_ID", "AZURE_CLIENT_ID", "AZURE_CLIENT_SECRET", "AZURE_SIGNING_ENDPOINT", "AZURE_SIGNING_ACCOUNT", "AZURE_SIGNING_PROFILE")
) {
  provider = "Azure Artifact Signing";
  // jsign tự đọc AZURE_TENANT_ID / AZURE_CLIENT_ID / AZURE_CLIENT_SECRET; dịch vụ tự gắn timestamp
  args = [
    "--storetype", "TRUSTEDSIGNING",
    "--keystore", env.AZURE_SIGNING_ENDPOINT.replace(/^https?:\/\//, "").replace(/\/+$/, ""),
    "--alias", `${env.AZURE_SIGNING_ACCOUNT}/${env.AZURE_SIGNING_PROFILE}`,
  ];
} else if (has("WINDOWS_CERTIFICATE", "WINDOWS_CERTIFICATE_PASSWORD")) {
  provider = "PFX";
  tempDir = mkdtempSync(join(env.RUNNER_TEMP || tmpdir(), "aio-sign-"));
  const pfx = join(tempDir, "cert.pfx");
  writeFileSync(pfx, Buffer.from(env.WINDOWS_CERTIFICATE.replace(/\s+/g, ""), "base64"), { mode: 0o600 });
  args = ["--keystore", pfx, "--storetype", "PKCS12", "--storepass", env.WINDOWS_CERTIFICATE_PASSWORD];
  timestamp(args, "http://timestamp.sectigo.com");
} else {
  console.error("[sign] Không có thông tin ký số nào (eSigner / Azure / PFX). Xem SIGNING.md.");
  process.exit(2);
}

function timestamp(a, fallback) {
  const url = env.WINDOWS_TIMESTAMP_URL || fallback;
  if (url !== "none") a.push("--tsaurl", url, "--tsmode", "RFC3161", "--tsretries", "5");
}

console.log(`[sign] ${provider}: ${file}`);
const res = spawnSync("java", ["-jar", jar, ...args, ...common, file], {
  stdio: ["ignore", "pipe", "pipe"],
  encoding: "utf8",
});
if (tempDir) rmSync(tempDir, { recursive: true, force: true });

// Không in lại lệnh hay tham số: chúng chứa mật khẩu
const out = `${res.stdout ?? ""}${res.stderr ?? ""}`.trim();
if (res.error) {
  console.error(`[sign] Không chạy được java: ${res.error.message}`);
  process.exit(1);
}
if (res.status !== 0) {
  console.error(`[sign] Ký thất bại (mã ${res.status})\n${out}`);
  process.exit(res.status ?? 1);
}
if (out) console.log(out);
console.log("[sign] OK");
