import type { ReactNode } from "react";
import { Button, Dropdown, Tooltip, Typography } from "antd";
import {
  ArrowRightOutlined,
  CaretRightOutlined,
  CheckCircleFilled,
  ClockCircleOutlined,
  CloseCircleFilled,
  CloudServerOutlined,
  DeleteOutlined,
  EditOutlined,
  ExclamationCircleFilled,
  EyeOutlined,
  FolderOpenOutlined,
  MinusCircleFilled,
  MoreOutlined,
  PauseCircleFilled,
  RollbackOutlined,
  StopOutlined,
  SyncOutlined,
  ThunderboltOutlined,
  UsbOutlined,
} from "@ant-design/icons";
import dayjs from "dayjs";
import { bytes, ftpUrl, reportStatus, type HistoryEntry, type Job } from "../api";
import type { JobActions } from "../App";
import type { AppData } from "../useAppData";
import { Counts, everyLabel, RunProgress, scheduleLabel, TimeAgo } from "./common";

/** Đích sẵn sàng để chạy: FTP luôn thử được, USB phải đang cắm. */
export function isReady(job: Job, data: AppData) {
  return job.destination.kind === "ftp" || data.presentTargets.has(job.destination.target_id);
}

export type JobState = "running" | "ok" | "error" | "cancelled" | "never" | "waiting";

export function jobState(job: Job, data: AppData): JobState {
  if (data.running[job.id]) return "running";
  const last = data.lastRunByJob.get(job.id);
  if (!last) return isReady(job, data) ? "never" : "waiting";
  return reportStatus(last.report);
}

const stateView: Record<JobState, { icon: ReactNode; label: string }> = {
  running: { icon: <SyncOutlined spin />, label: "Đang chạy" },
  ok: { icon: <CheckCircleFilled />, label: "Đã bảo vệ" },
  error: { icon: <CloseCircleFilled />, label: "Lỗi" },
  cancelled: { icon: <MinusCircleFilled />, label: "Đã huỷ" },
  never: { icon: <ExclamationCircleFilled />, label: "Chưa backup" },
  waiting: { icon: <PauseCircleFilled />, label: "Chờ cắm ổ" },
};

export function StatePill({ state, why }: { state: JobState; why?: string }) {
  const v = stateView[state];
  const pill = (
    <span className={`pill tone-${state}`}>
      {v.icon}
      {v.label}
    </span>
  );
  return why ? <Tooltip title={why}>{pill}</Tooltip> : pill;
}

function errorReason(e: HistoryEntry | undefined) {
  const r = e?.report;
  if (!r || reportStatus(r) !== "error") return undefined;
  return r.error ?? (r.targetLost ? "Mất kết nối tới đích giữa chừng" : `${r.failed.length} file không backup được`);
}

/** Dải chấm kết quả các lượt chạy gần đây, cũ bên trái, mới bên phải. */
export function RunDots({ runs, slots = 12 }: { runs: HistoryEntry[]; slots?: number }) {
  const shown = runs.slice(0, slots).reverse();
  const pad = slots - shown.length;
  return (
    <div className="run-dots" aria-label={`${runs.length} lượt chạy gần đây`}>
      {Array.from({ length: pad }, (_, i) => (
        <span key={`pad${i}`} className="run-dot empty" />
      ))}
      {shown.map((e, i) => {
        const s = reportStatus(e.report);
        const r = e.report;
        const label = s === "ok" ? "Thành công" : s === "cancelled" ? "Đã huỷ" : "Lỗi";
        return (
          <Tooltip
            key={i}
            title={
              <>
                <b>{label}</b> · {dayjs(r.finishedAt).format("HH:mm DD/MM/YYYY")}
                <br />
                {r.error ?? `+${r.created} ~${r.updated} -${r.deleted} · ${bytes(r.bytesCopied)}`}
              </>
            }
          >
            <span className={`run-dot tone-${s}`} />
          </Tooltip>
        );
      })}
    </div>
  );
}

export function Triggers({ job }: { job: Job }) {
  const items: ReactNode[] = [];
  if (job.destination.kind === "usb" && job.auto_run)
    items.push(
      <span key="plug" className="chip">
        <ThunderboltOutlined /> Khi cắm ổ
      </span>,
    );
  if (job.interval_minutes)
    items.push(
      <span key="every" className="chip">
        <ClockCircleOutlined /> {everyLabel(job.interval_minutes)}
      </span>,
    );
  if (job.schedule)
    items.push(
      <span key="at" className="chip">
        <ClockCircleOutlined /> {scheduleLabel(job.schedule)}
      </span>,
    );
  if (items.length === 0) items.push(<span key="manual" className="chip">Thủ công</span>);
  return <div className="chips">{items}</div>;
}

/** Nguồn → đích. */
function Flow({ job, data }: { job: Job; data: AppData }) {
  const d = job.destination;
  const drive = d.kind === "usb" ? data.presentTargets.get(d.target_id) : undefined;
  const destText = d.kind === "ftp" ? ftpUrl(d) : drive ? `${drive.marker!.label} (${drive.mount_point})` : "Ổ chưa cắm";
  return (
    <div className="flow">
      <div className="flow-end">
        <FolderOpenOutlined />
        <Typography.Text className="mono" ellipsis={{ tooltip: job.source }}>
          {job.source}
        </Typography.Text>
      </div>
      <ArrowRightOutlined className="flow-arrow" />
      <div className={d.kind === "usb" && !drive ? "flow-end missing" : "flow-end"}>
        {d.kind === "ftp" ? <CloudServerOutlined /> : <UsbOutlined />}
        <Typography.Text className={d.kind === "ftp" ? "mono" : undefined} ellipsis={{ tooltip: destText }}>
          {destText}
        </Typography.Text>
      </div>
    </div>
  );
}

function nextLabel(job: Job, data: AppData): ReactNode {
  const s = data.scheduleByJob.get(job.id);
  if (data.running[job.id]) return "Đang chạy";
  if (s?.nextRun) return dayjs(s.nextRun).diff(dayjs(), "second") < 30 ? "Sắp chạy" : <TimeAgo iso={s.nextRun} />;
  if (job.destination.kind === "usb") {
    if (!s?.available) return "Chờ cắm ổ";
    return job.auto_run ? "Lần cắm ổ sau" : "Thủ công";
  }
  return "Thủ công";
}

function Meta({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="meta">
      <span className="meta-label">{label}</span>
      <span className="meta-value">{children}</span>
    </div>
  );
}

export default function JobCard({
  job,
  data,
  actions,
  compact,
}: {
  job: Job;
  data: AppData;
  actions: JobActions;
  compact?: boolean;
}) {
  const state = jobState(job, data);
  const run = data.running[job.id];
  const last = data.lastRunByJob.get(job.id);
  const ready = isReady(job, data);
  const preview = data.results[job.id]?.report?.dryRun ? data.results[job.id]!.report! : null;

  const runButton = run ? (
    <Button size={compact ? "small" : "middle"} icon={<StopOutlined />} onClick={() => actions.cancel(job)}>
      Huỷ
    </Button>
  ) : (
    <Tooltip title={ready ? "" : "Ổ đích chưa cắm"}>
      <Button
        type={compact ? "default" : "primary"}
        size={compact ? "small" : "middle"}
        icon={<CaretRightOutlined />}
        disabled={!ready}
        onClick={() => actions.run(job, false)}
      >
        Backup
      </Button>
    </Tooltip>
  );

  return (
    <div className={compact ? "job-card compact" : "job-card"} data-state={state}>
      <div className="job-card-head">
        <div className="job-icon">{job.destination.kind === "ftp" ? <CloudServerOutlined /> : <UsbOutlined />}</div>
        <div className="job-title">
          <Typography.Text strong ellipsis={{ tooltip: job.name }} className="job-name">
            {job.name}
          </Typography.Text>
          {compact ? (
            <Typography.Text type="secondary" className="mono job-sub" ellipsis={{ tooltip: job.source }}>
              {job.source}
            </Typography.Text>
          ) : (
            <Triggers job={job} />
          )}
        </div>
        <StatePill state={state} why={errorReason(last)} />
      </div>

      {!compact && <Flow job={job} data={data} />}

      {run ? (
        <RunProgress run={run} />
      ) : (
        <div className="job-meta">
          <Meta label="Lần cuối">
            {last ? <TimeAgo iso={last.report.finishedAt} /> : "Chưa chạy"}
          </Meta>
          <Meta label="Kế tiếp">{nextLabel(job, data)}</Meta>
          {!compact && last && (
            <Meta label="Thay đổi">
              <Counts report={last.report} />
            </Meta>
          )}
        </div>
      )}

      {!compact && preview && !run && (
        <div className="job-preview">
          <EyeOutlined /> Xem trước: sẽ +{preview.created} ~{preview.updated} -{preview.deleted} ·{" "}
          {bytes(preview.bytesCopied)}
        </div>
      )}

      <div className="job-card-foot">
        <RunDots runs={data.recentRunsByJob.get(job.id) ?? []} slots={compact ? 10 : 12} />
        <div className="job-actions">
          {!compact && !run && (
            <Tooltip title="Xem trước: liệt kê thay đổi, không ghi gì">
              <Button icon={<EyeOutlined />} disabled={!ready} onClick={() => actions.run(job, true)} />
            </Tooltip>
          )}
          {runButton}
          {!compact && (
            <Dropdown
              trigger={["click"]}
              menu={{
                items: [
                  { key: "edit", icon: <EditOutlined />, label: "Sửa" },
                  { key: "restore", icon: <RollbackOutlined />, label: "Khôi phục file" },
                  { type: "divider" },
                  { key: "delete", icon: <DeleteOutlined />, label: "Xoá job", danger: true },
                ],
                onClick: ({ key }) => {
                  if (key === "edit") actions.edit(job);
                  if (key === "restore") actions.restore(job);
                  if (key === "delete") actions.remove(job);
                },
              }}
            >
              <Button icon={<MoreOutlined />} />
            </Dropdown>
          )}
        </div>
      </div>
    </div>
  );
}
