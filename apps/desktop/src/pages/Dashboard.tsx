import { useMemo, type ReactNode } from "react";
import { Alert, Button, Card, Col, Empty, Row, Space, Table, theme, Typography } from "antd";
import {
  CaretRightOutlined,
  CheckCircleOutlined,
  CloudUploadOutlined,
  PlusOutlined,
  SyncOutlined,
  UnorderedListOutlined,
} from "@ant-design/icons";
import dayjs from "dayjs";
import { bytes, reportStatus, type Job } from "../api";
import type { PageProps } from "../App";
import { DestLabel, RunProgress, StatusTag, TimeAgo } from "../components/common";

function StatCard(props: { icon: ReactNode; color: string; title: string; value: ReactNode; sub: ReactNode }) {
  return (
    <Card className="stat-card">
      <div
        className="stat-icon"
        style={{ color: props.color, background: `color-mix(in srgb, ${props.color} 14%, transparent)` }}
      >
        {props.icon}
      </div>
      <div className="stat-body">
        <div className="stat-title">{props.title}</div>
        <div className="stat-value">{props.value}</div>
        <div className="stat-sub">{props.sub}</div>
      </div>
    </Card>
  );
}

export default function Dashboard({ data, actions, goto }: PageProps) {
  const { token } = theme.useToken();
  const { config, history, running, schedule, lastRunByJob, presentTargets } = data;
  const jobs = config.jobs;
  const scheduleBy = useMemo(() => new Map(schedule.map((s) => [s.jobId, s])), [schedule]);

  const stats = useMemo(() => {
    const weekAgo = dayjs().subtract(7, "day");
    const real = history.filter((e) => !e.report.dryRun);
    const week = real.filter((e) => dayjs(e.report.startedAt).isAfter(weekAgo));
    const last = real.find((e) => reportStatus(e.report) === "ok");
    return {
      usb: jobs.filter((j) => j.destination.kind === "usb").length,
      ftp: jobs.filter((j) => j.destination.kind === "ftp").length,
      weekBytes: week.reduce((s, e) => s + (e.report.bytesCopied || 0), 0),
      weekRuns: week.length,
      weekErrors: week.filter((e) => reportStatus(e.report) === "error").length,
      last,
    };
  }, [history, jobs]);

  // Job mà lượt chạy thật gần nhất bị lỗi
  const failing = jobs.filter((j) => {
    const e = lastRunByJob.get(j.id);
    return e && reportStatus(e.report) === "error";
  });
  const runningCount = Object.keys(running).length;

  const nextLabel = (job: Job) => {
    const s = scheduleBy.get(job.id);
    if (running[job.id]) return <Typography.Text type="success">Đang chạy</Typography.Text>;
    if (s?.nextRun) {
      const t = dayjs(s.nextRun);
      return t.diff(dayjs(), "second") < 30 ? "Sắp chạy" : <TimeAgo iso={s.nextRun} />;
    }
    if (job.destination.kind === "usb") {
      if (!s?.available) return <Typography.Text type="secondary">Chờ cắm ổ</Typography.Text>;
      return <Typography.Text type="secondary">{job.auto_run ? "Lần cắm ổ sau" : "Thủ công"}</Typography.Text>;
    }
    return <Typography.Text type="secondary">Thủ công</Typography.Text>;
  };

  return (
    <div className="page">
      <div className="page-head">
        <h1>Tổng quan</h1>
        <Button type="primary" icon={<PlusOutlined />} onClick={actions.create}>
          Thêm job
        </Button>
      </div>

      <Row gutter={[12, 12]}>
        <Col xs={12} md={6}>
          <StatCard
            icon={<UnorderedListOutlined />}
            color={token.colorPrimary}
            title="Job backup"
            value={jobs.length}
            sub={`${stats.usb} ổ đĩa · ${stats.ftp} FTP`}
          />
        </Col>
        <Col xs={12} md={6}>
          <StatCard
            icon={<SyncOutlined spin={runningCount > 0} />}
            color={token.purple}
            title="Đang chạy"
            value={runningCount}
            sub={`${presentTargets.size} ổ đích đang cắm`}
          />
        </Col>
        <Col xs={12} md={6}>
          <StatCard
            icon={<CheckCircleOutlined />}
            color={token.colorSuccess}
            title="Thành công gần nhất"
            value={stats.last ? dayjs(stats.last.report.finishedAt).fromNow() : "Chưa có"}
            sub={stats.last?.jobName ?? "—"}
          />
        </Col>
        <Col xs={12} md={6}>
          <StatCard
            icon={<CloudUploadOutlined />}
            color={token.colorWarning}
            title="7 ngày qua"
            value={bytes(stats.weekBytes)}
            sub={
              <Typography.Text type={stats.weekErrors ? "danger" : "secondary"} style={{ fontSize: 12 }}>
                {stats.weekRuns} lượt chạy{stats.weekErrors ? ` · ${stats.weekErrors} lỗi` : ""}
              </Typography.Text>
            }
          />
        </Col>
      </Row>

      {failing.length > 0 && (
        <Alert
          type="error"
          showIcon
          style={{ marginTop: 16 }}
          title={`${failing.length} job có lượt chạy gần nhất bị lỗi`}
          description={failing.map((j) => {
            const r = lastRunByJob.get(j.id)!.report;
            return (
              <div key={j.id}>
                <b>{j.name}</b>:{" "}
                {r.error ?? (r.targetLost ? "mất kết nối tới đích" : `${r.failed.length} file lỗi`)}
              </div>
            );
          })}
          action={
            <Button size="small" onClick={() => goto("history")}>
              Xem lịch sử
            </Button>
          }
        />
      )}

      <Card title="Tình trạng job" style={{ marginTop: 12 }} styles={{ body: { padding: 0 } }}>
        {jobs.length === 0 ? (
          <Empty
            style={{ padding: 32 }}
            description="Chưa có job nào. Cắm ổ USB và đánh dấu làm ổ đích, hoặc thêm job FTP."
          >
            <Button type="primary" onClick={actions.create}>
              Thêm job đầu tiên
            </Button>
          </Empty>
        ) : (
          <Table<Job>
            rowKey="id"
            dataSource={jobs}
            pagination={false}
            tableLayout="fixed"
            columns={[
              {
                title: "Job",
                render: (_, j) => (
                  <div className="cell-stack">
                    <Typography.Text strong>{j.name}</Typography.Text>
                    <DestLabel
                      job={j}
                      drive={j.destination.kind === "usb" ? presentTargets.get(j.destination.target_id) : undefined}
                    />
                  </div>
                ),
              },
              {
                title: "Lần chạy gần nhất",
                width: 220,
                render: (_, j) => {
                  if (running[j.id]) return <RunProgress run={running[j.id]} />;
                  const e = lastRunByJob.get(j.id);
                  if (!e) return <Typography.Text type="secondary">Chưa chạy</Typography.Text>;
                  return (
                    <Space size={4}>
                      <StatusTag report={e.report} />
                      <TimeAgo iso={e.report.finishedAt} />
                    </Space>
                  );
                },
              },
              { title: "Lần kế tiếp", width: 150, render: (_, j) => nextLabel(j) },
              {
                title: "",
                width: 90,
                align: "right",
                render: (_, j) => {
                  const ready = j.destination.kind === "ftp" || scheduleBy.get(j.id)?.available;
                  return running[j.id] ? (
                    <Button size="small" onClick={() => actions.cancel(j)}>
                      Huỷ
                    </Button>
                  ) : (
                    <Button
                      size="small"
                      icon={<CaretRightOutlined />}
                      disabled={!ready}
                      onClick={() => actions.run(j, false)}
                    >
                      Chạy
                    </Button>
                  );
                },
              },
            ]}
          />
        )}
      </Card>
    </div>
  );
}
