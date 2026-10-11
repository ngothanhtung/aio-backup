// Kiểu dữ liệu khớp với struct Rust (serde) và wrapper gọi command.
import { invoke } from "@tauri-apps/api/core";

export type Uuid = string;

export interface TargetMarker {
  id: Uuid;
  label: string;
  created_at: string;
  format: number;
}

export interface DriveInfo {
  name: string;
  mount_point: string;
  file_system: string;
  total_space: number;
  available_space: number;
  removable: boolean;
  marker: TargetMarker | null;
}

export interface FtpTarget {
  host: string;
  port: number;
  username: string;
  remote_dir: string;
  secure: boolean;
  accept_invalid_certs: boolean;
}

export type Destination = { kind: "usb"; target_id: Uuid } | ({ kind: "ftp" } & FtpTarget);

export interface Job {
  id: Uuid;
  name: string;
  source: string;
  destination: Destination;
  dest_subdir: string;
  excludes: string[];
  mirror_deletes: boolean;
  versioning: boolean;
  keep_versions_days: number | null;
  verify_hash: boolean;
  auto_run: boolean;
  interval_minutes: number | null;
}

export interface Config {
  poll_interval_secs: number;
  jobs: Job[];
}

export type ActionKind = "create" | "update" | "delete";

export type Progress =
  | { type: "started"; totalFiles: number; totalBytes: number }
  | { type: "file"; rel: string; kind: ActionKind; doneFiles: number; doneBytes: number }
  | { type: "warning"; message: string };

export interface Report {
  startedAt: string;
  finishedAt: string;
  dryRun: boolean;
  created: number;
  updated: number;
  deleted: number;
  unchanged: number;
  bytesCopied: number;
  failed: { rel: string; error: string }[];
  warnings: string[];
  cancelled: boolean;
  targetLost: boolean;
}

export interface HistoryEntry {
  jobId: Uuid;
  jobName: string;
  report: Report;
}

export interface ProgressEvent {
  jobId: Uuid;
  progress: Progress;
}

export interface FinishedEvent {
  jobId: Uuid;
  jobName: string;
  report: Report | null;
  error: string | null;
}

export const api = {
  getConfig: () => invoke<Config>("get_config"),
  getDrives: () => invoke<DriveInfo[]>("get_drives"),
  markTarget: (path: string, label: string, force = false) =>
    invoke<TargetMarker>("mark_target", { path, label, force }),
  saveJob: (job: Job, password?: string) =>
    invoke<{ config: Config; warning: string | null }>("save_job", { job, password: password || null }),
  testFtp: (ftp: FtpTarget, password: string, jobId?: Uuid) =>
    invoke<string>("test_ftp", { ftp, password: password || null, jobId: jobId ?? null }),
  deleteJob: (id: Uuid) => invoke<Config>("delete_job", { id }),
  startJob: (id: Uuid, dryRun: boolean) => invoke<void>("start_job", { id, dryRun }),
  cancelJob: (id: Uuid) => invoke<void>("cancel_job", { id }),
  runningJobs: () => invoke<Uuid[]>("running_jobs"),
  getHistory: (limit = 30) => invoke<HistoryEntry[]>("get_history", { limit }),
};

export const emptyFtp = (): FtpTarget => ({
  host: "",
  port: 21,
  username: "",
  remote_dir: "/backup",
  secure: true,
  accept_invalid_certs: false,
});

/** Job mới: đích USB nếu đã có ổ đích, ngược lại FTP. */
export function newJob(usbTargetId?: Uuid): Job {
  return {
    id: crypto.randomUUID(),
    name: "",
    source: "",
    destination: usbTargetId
      ? { kind: "usb", target_id: usbTargetId }
      : { kind: "ftp", ...emptyFtp() },
    dest_subdir: "",
    excludes: ["node_modules", ".git", "*.tmp"],
    mirror_deletes: false,
    versioning: true,
    keep_versions_days: 30,
    verify_hash: false,
    auto_run: true,
    interval_minutes: usbTargetId ? null : 60,
  };
}

export function ftpUrl(f: FtpTarget): string {
  const dir = f.remote_dir.replace(/^\/+|\/+$/g, "");
  return `${f.secure ? "ftps" : "ftp"}://${f.username}@${f.host}${f.port === 21 ? "" : `:${f.port}`}${dir ? `/${dir}` : ""}`;
}

export function bytes(n: number): string {
  const u = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  while (n >= 1024 && i < u.length - 1) {
    n /= 1024;
    i++;
  }
  return `${n.toFixed(i === 0 ? 0 : 1)} ${u[i]}`;
}

export function errText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : JSON.stringify(e);
}
