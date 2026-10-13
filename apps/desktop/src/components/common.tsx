import { Progress, Space, Tag, Tooltip, Typography } from "antd";
import {
  CheckCircleFilled,
  CloseCircleFilled,
  CloudServerOutlined,
  EyeOutlined,
  MinusCircleFilled,
  SyncOutlined,
  UsbOutlined,
} from "@ant-design/icons";
import dayjs from "dayjs";
import { bytes, ftpUrl, reportStatus, type DriveInfo, type Job, type Report, type Schedule } from "../api";
import type { RunState } from "../useAppData";

/** Nhãn đích: USB (tên ổ) hoặc FTP (url). */
export function DestLabel({ job, drive }: { job: Job; drive?: DriveInfo }) {
  const d = job.destination;
  if (d.kind === "ftp") {
    return (
      <div className="dest-label">
        <Tag icon={<CloudServerOutlined />} color="geekblue" style={{ marginInlineEnd: 0 }}>
          {d.secure ? "FTPS" : "FTP"}
        </Tag>
        <Typography.Text type="secondary" className="mono" ellipsis={{ tooltip: ftpUrl(d) }}>
          {ftpUrl(d)}
        </Typography.Text>
      </div>
    );
  }
  return (
    <div className="dest-label">
      <Tag icon={<UsbOutlined />} color={drive ? "green" : "default"} style={{ marginInlineEnd: 0 }}>
        USB
      </Tag>
      <Typography.Text type="secondary" ellipsis>
        {drive ? `${drive.marker!.label} (${drive.mount_point})` : "Ổ chưa cắm"}
      </Typography.Text>
    </div>
  );
}

export function StatusTag({ report }: { report: Report }) {
  const s = reportStatus(report);
  if (s === "ok" && report.dryRun)
    return (
      <Tag color="blue" icon={<EyeOutlined />}>
        Xem trước
      </Tag>
    );
  if (s === "ok")
    return (
      <Tag color="success" icon={<CheckCircleFilled />}>
        Thành công
      </Tag>
    );
  if (s === "cancelled")
    return (
      <Tag color="default" icon={<MinusCircleFilled />}>
        Đã huỷ
      </Tag>
    );
  const why = report.error
    ? report.error
    : report.targetLost
      ? "Mất kết nối tới đích giữa chừng"
      : `${report.failed.length} file lỗi`;
  return (
    <Tooltip title={why}>
      <Tag color="error" icon={<CloseCircleFilled />}>
        Lỗi
      </Tag>
    </Tooltip>
  );
}

/** Tóm tắt số file: +mới ~cập nhật -xoá. */
export function Counts({ report }: { report: Report }) {
  if (report.error) {
    return (
      <Typography.Text type="danger" ellipsis={{ tooltip: report.error }} style={{ maxWidth: 200 }}>
        {report.error}
      </Typography.Text>
    );
  }
  return (
    <Space size={8}>
      <Typography.Text type="success">+{report.created}</Typography.Text>
      <Typography.Text style={{ color: "#d48806" }}>~{report.updated}</Typography.Text>
      <Typography.Text type="danger">-{report.deleted}</Typography.Text>
    </Space>
  );
}

export function RunProgress({ run }: { run: RunState }) {
  const pct = run.totalBytes > 0 ? Math.floor((run.doneBytes / run.totalBytes) * 100) : 0;
  return (
    <div style={{ minWidth: 180 }}>
      <Progress percent={pct} size="small" status="active" />
      <Typography.Text type="secondary" style={{ fontSize: 12 }} ellipsis={{ tooltip: run.current }}>
        <SyncOutlined spin /> {run.dryRun ? "Xem trước · " : ""}
        {run.doneFiles}/{run.totalFiles} file · {bytes(run.doneBytes)}/{bytes(run.totalBytes)}
      </Typography.Text>
    </div>
  );
}

export function TimeAgo({ iso }: { iso: string | null | undefined }) {
  if (!iso) return <Typography.Text type="secondary">—</Typography.Text>;
  const t = dayjs(iso);
  return <Tooltip title={t.format("HH:mm:ss DD/MM/YYYY")}>{t.fromNow()}</Tooltip>;
}

export function duration(r: Report): string {
  const ms = dayjs(r.finishedAt).diff(dayjs(r.startedAt));
  if (ms < 1000) return "< 1 giây";
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s} giây`;
  const m = Math.floor(s / 60);
  return m < 60 ? `${m} phút ${s % 60} giây` : `${Math.floor(m / 60)} giờ ${m % 60} phút`;
}

/** "Mỗi 90 phút" / "Mỗi 2 giờ" / "Mỗi ngày". */
export const WEEKDAYS = ["T2", "T3", "T4", "T5", "T6", "T7", "CN"];

export function scheduleLabel(s: Schedule): string {
  if (s.every === "daily") return `Hằng ngày ${s.at}`;
  if (s.every === "monthly") return `Ngày ${s.day} hằng tháng ${s.at}`;
  const days = [...s.weekdays].sort((a, b) => a - b).map((d) => WEEKDAYS[d - 1]);
  return `${days.join(", ")} ${s.at}`;
}

export function everyLabel(min: number): string {
  if (min % 1440 === 0) return min === 1440 ? "Mỗi ngày" : `Mỗi ${min / 1440} ngày`;
  if (min % 60 === 0) return `Mỗi ${min / 60} giờ`;
  return `Mỗi ${min} phút`;
}
