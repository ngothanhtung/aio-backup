import { useMemo } from "react";
import { Alert, Button, Card, Col, Empty, Row, Space, Statistic, Table, Typography } from "antd";
import { CaretRightOutlined, PlusOutlined } from "@ant-design/icons";
import dayjs from "dayjs";
import { bytes, reportStatus, type Job } from "../api";
import type { PageProps } from "../App";
import { DestLabel, RunProgress, StatusTag, TimeAgo } from "../components/common";

export default function Dashboard({ data, actions, goto }: PageProps) {
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

      <Row gutter={[16, 16]}>
        <Col xs={12} md={6}>
          <Card style={{ height: "100%" }}>
            <Statistic title="Job backup" value={jobs.length} />
            <Typography.Text type="secondary">
              {stats.usb} USB · {stats.ftp} FTP
            </Typography.Text>
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card style={{ height: "100%" }}>
            <Statistic
              title="Đang chạy"
              value={runningCount}
              styles={{ content: runningCount ? { color: "#1f6feb" } : undefined }}
            />
            <Typography.Text type="secondary">
              {presentTargets.size} ổ đích đang cắm
            </Typography.Text>
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card style={{ height: "100%" }}>
            <Statistic
              title="Backup thành công gần nhất"
              value={stats.last ? dayjs(stats.last.report.finishedAt).fromNow() : "Chưa có"}
              styles={{ content: { fontSize: 20 } }}
            />
            <Typography.Text type="secondary" ellipsis>
              {stats.last?.jobName ?? "—"}
            </Typography.Text>
          </Card>
        </Col>
        <Col xs={12} md={6}>
          <Card style={{ height: "100%" }}>
            <Statistic title="7 ngày qua" value={bytes(stats.weekBytes)} styles={{ content: { fontSize: 20 } }} />
            <Typography.Text type={stats.weekErrors ? "danger" : "secondary"}>
              {stats.weekRuns} lượt chạy{stats.weekErrors ? ` · ${stats.weekErrors} lỗi` : ""}
            </Typography.Text>
          </Card>
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

      <Card title="Tình trạng job" style={{ marginTop: 16 }} styles={{ body: { padding: 0 } }}>
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
