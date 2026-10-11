import { useMemo, useState } from "react";
import {
  Alert,
  Button,
  DatePicker,
  Descriptions,
  Drawer,
  Empty,
  Select,
  Space,
  Table,
  Typography,
} from "antd";
import { ReloadOutlined } from "@ant-design/icons";
import dayjs, { type Dayjs } from "dayjs";
import { bytes, reportStatus, type HistoryEntry } from "../api";
import type { PageProps } from "../App";
import { Counts, duration, StatusTag } from "../components/common";

type StatusFilter = "all" | "ok" | "error" | "dry";

export default function History({ data }: PageProps) {
  const { history, config, refreshHistory } = data;
  const [jobId, setJobId] = useState<string | undefined>();
  const [status, setStatus] = useState<StatusFilter>("all");
  const [range, setRange] = useState<[Dayjs | null, Dayjs | null] | null>(null);
  const [detail, setDetail] = useState<HistoryEntry | null>(null);

  // Gồm cả job đã xoá nhưng còn trong lịch sử
  const jobOptions = useMemo(() => {
    const m = new Map<string, string>();
    config.jobs.forEach((j) => m.set(j.id, j.name));
    history.forEach((e) => m.has(e.jobId) || m.set(e.jobId, e.jobName));
    return [...m].map(([value, label]) => ({ value, label }));
  }, [config.jobs, history]);

  const rows = useMemo(
    () =>
      history
        .map((e, i) => ({ ...e, key: i }))
        .filter((e) => !jobId || e.jobId === jobId)
        .filter((e) => {
          const s = reportStatus(e.report);
          if (status === "dry") return e.report.dryRun;
          if (status === "ok") return !e.report.dryRun && s === "ok";
          if (status === "error") return s === "error";
          return true;
        })
        .filter((e) => {
          if (!range) return true;
          const t = dayjs(e.report.startedAt);
          return (!range[0] || !t.isBefore(range[0].startOf("day"))) && (!range[1] || !t.isAfter(range[1].endOf("day")));
        }),
    [history, jobId, status, range],
  );

  return (
    <div className="page">
      <div className="page-head">
        <h1>Lịch sử</h1>
        <Button icon={<ReloadOutlined />} onClick={refreshHistory}>
          Làm mới
        </Button>
      </div>

      <Space wrap style={{ marginBottom: 16 }}>
        <Select
          allowClear
          placeholder="Tất cả job"
          style={{ width: 200 }}
          options={jobOptions}
          value={jobId}
          onChange={setJobId}
        />
        <Select<StatusFilter>
          style={{ width: 160 }}
          value={status}
          onChange={setStatus}
          options={[
            { value: "all", label: "Mọi kết quả" },
            { value: "ok", label: "Thành công" },
            { value: "error", label: "Có lỗi" },
            { value: "dry", label: "Xem trước" },
          ]}
        />
        <DatePicker.RangePicker value={range} onChange={(v) => setRange(v)} format="DD/MM/YYYY" />
      </Space>

      <Table
        dataSource={rows}
        size="middle"
        tableLayout="fixed"
        pagination={{ pageSize: 20, showSizeChanger: false, showTotal: (n) => `${n} lượt` }}
        locale={{ emptyText: <Empty description="Không có lượt chạy nào" /> }}
        rowClassName={() => "clickable-row"}
        onRow={(r) => ({ onClick: () => setDetail(r) })}
        columns={[
          {
            title: "Thời điểm",
            width: 170,
            render: (_, e) => dayjs(e.report.startedAt).format("HH:mm:ss DD/MM/YYYY"),
          },
          { title: "Job", dataIndex: "jobName", ellipsis: true },
          { title: "Kết quả", width: 130, render: (_, e) => <StatusTag report={e.report} /> },
          { title: "Thay đổi", width: 220, render: (_, e) => <Counts report={e.report} /> },
          { title: "Dung lượng", width: 110, align: "right", render: (_, e) => bytes(e.report.bytesCopied || 0) },
          { title: "Thời gian", width: 120, render: (_, e) => duration(e.report) },
        ]}
      />

      <Drawer
        open={!!detail}
        size={560}
        title={detail ? `${detail.jobName} · ${dayjs(detail.report.startedAt).format("HH:mm DD/MM/YYYY")}` : ""}
        onClose={() => setDetail(null)}
      >
        {detail && <RunDetail entry={detail} />}
      </Drawer>
    </div>
  );
}

function RunDetail({ entry }: { entry: HistoryEntry }) {
  const r = entry.report;
  return (
    <Space orientation="vertical" size={16} style={{ width: "100%" }}>
      {r.error && <Alert type="error" showIcon title="Không chạy được" description={r.error} />}
      {r.targetLost && <Alert type="error" showIcon title="Mất kết nối tới đích giữa chừng" />}
      <Descriptions
        column={2}
        size="small"
        bordered
        items={[
          { label: "Kết quả", children: <StatusTag report={r} /> },
          { label: "Kiểu", children: r.dryRun ? "Xem trước" : "Chạy thật" },
          { label: "Bắt đầu", children: dayjs(r.startedAt).format("HH:mm:ss DD/MM/YYYY") },
          { label: "Thời gian", children: duration(r) },
          { label: "File mới", children: r.created },
          { label: "Cập nhật", children: r.updated },
          { label: "Đã xoá", children: r.deleted },
          { label: "Không đổi", children: r.unchanged },
          { label: "Dung lượng", children: bytes(r.bytesCopied || 0), span: 2 },
        ]}
      />
      {r.failed.length > 0 && (
        <div>
          <Typography.Title level={5}>File lỗi ({r.failed.length})</Typography.Title>
          <Table
            size="small"
            rowKey={(f) => f.rel}
            dataSource={r.failed}
            pagination={r.failed.length > 10 ? { pageSize: 10 } : false}
            columns={[
              { title: "File", dataIndex: "rel", render: (v) => <span className="mono">{v}</span> },
              { title: "Lỗi", dataIndex: "error" },
            ]}
          />
        </div>
      )}
      {r.warnings.length > 0 && (
        <div>
          <Typography.Title level={5}>Cảnh báo ({r.warnings.length})</Typography.Title>
          <ul style={{ paddingLeft: 18, margin: 0 }}>
            {r.warnings.slice(0, 200).map((w, i) => (
              <li key={i}>
                <Typography.Text type="secondary">{w}</Typography.Text>
              </li>
            ))}
          </ul>
          {r.warnings.length > 200 && (
            <Typography.Text type="secondary">… và {r.warnings.length - 200} cảnh báo khác</Typography.Text>
          )}
        </div>
      )}
      {!r.error && r.failed.length === 0 && r.warnings.length === 0 && (
        <Typography.Text type="secondary">Không có lỗi hay cảnh báo.</Typography.Text>
      )}
    </Space>
  );
}
