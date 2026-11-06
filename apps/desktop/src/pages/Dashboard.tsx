import { useMemo, type ReactNode } from "react";
import { Button, Col, Empty, Progress, Row, theme, Tooltip, Typography } from "antd";
import {
  CloudServerOutlined,
  CloudUploadOutlined,
  ExclamationCircleFilled,
  HddOutlined,
  PieChartOutlined,
  PlusOutlined,
  SafetyCertificateFilled,
  SafetyOutlined,
  SyncOutlined,
  UnorderedListOutlined,
  UsbOutlined,
  WarningFilled,
} from "@ant-design/icons";
import dayjs from "dayjs";
import { bytes, ftpUrl, reportStatus, type HistoryEntry } from "../api";
import type { PageProps } from "../App";
import { TimeAgo } from "../components/common";
import JobCard, { isReady, jobState } from "../components/JobCard";

function StatCard(props: { icon: ReactNode; color: string; title: string; value: ReactNode; sub: ReactNode }) {
  return (
    <div className="stat-card">
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
    </div>
  );
}

function Panel({ title, extra, children }: { title: string; extra?: ReactNode; children: ReactNode }) {
  return (
    <section className="panel">
      <header className="panel-head">
        <h3>{title}</h3>
        {extra}
      </header>
      {children}
    </section>
  );
}

const DAYS = 14;

/** Cột chồng số lượt chạy mỗi ngày theo kết quả. */
function ActivityChart({ history }: { history: HistoryEntry[] }) {
  const days = useMemo(() => {
    const start = dayjs().startOf("day").subtract(DAYS - 1, "day");
    const list = Array.from({ length: DAYS }, (_, i) => ({
      day: start.add(i, "day"),
      ok: 0,
      error: 0,
      cancelled: 0,
      bytes: 0,
    }));
    for (const e of history) {
      if (e.report.dryRun) continue;
      const i = dayjs(e.report.startedAt).startOf("day").diff(start, "day");
      if (i < 0 || i >= DAYS) continue;
      list[i][reportStatus(e.report)]++;
      list[i].bytes += e.report.bytesCopied || 0;
    }
    return list;
  }, [history]);

  const max = Math.max(1, ...days.map((d) => d.ok + d.error + d.cancelled));
  const total = days.reduce((s, d) => s + d.ok + d.error + d.cancelled, 0);
  const sum = days.reduce((s, d) => s + d.bytes, 0);
  const today = dayjs().startOf("day");

  return (
    <Panel
      title="Hoạt động 14 ngày"
      extra={
        <span className="panel-note">
          {total} lượt · {bytes(sum)}
        </span>
      }
    >
      <div className="chart" role="img" aria-label={`Số lượt backup mỗi ngày trong ${DAYS} ngày qua`}>
        {days.map((d) => {
          const n = d.ok + d.error + d.cancelled;
          const isToday = d.day.isSame(today);
          return (
            <Tooltip
              key={d.day.valueOf()}
              title={
                <>
                  <b>{d.day.format("dddd DD/MM")}</b>
                  <br />
                  {n === 0
                    ? "Không có lượt chạy"
                    : [
                        d.ok && `${d.ok} thành công`,
                        d.error && `${d.error} lỗi`,
                        d.cancelled && `${d.cancelled} đã huỷ`,
                      ]
                        .filter(Boolean)
                        .join(" · ")}
                  {d.bytes > 0 && (
                    <>
                      <br />
                      {bytes(d.bytes)} đã ghi
                    </>
                  )}
                </>
              }
            >
              <div className="chart-col">
                <div className="chart-track">
                  {n > 0 && (
                    <div className="chart-bar" style={{ height: `${(n / max) * 100}%` }}>
                      {d.error > 0 && <span className="seg tone-error" style={{ flexGrow: d.error }} />}
                      {d.cancelled > 0 && <span className="seg tone-cancelled" style={{ flexGrow: d.cancelled }} />}
                      {d.ok > 0 && <span className="seg tone-ok" style={{ flexGrow: d.ok }} />}
                    </div>
                  )}
                </div>
                <span className={isToday ? "chart-x today" : "chart-x"}>{d.day.format("D")}</span>
              </div>
            </Tooltip>
          );
        })}
      </div>
      <div className="legend">
        <span>
          <i className="tone-ok" /> Thành công
        </span>
        <span>
          <i className="tone-error" /> Lỗi
        </span>
        <span>
          <i className="tone-cancelled" /> Đã huỷ
        </span>
      </div>
    </Panel>
  );
}

/** Dung lượng các ổ đích đang cắm + máy chủ FTP đang dùng. */
function Targets({ data }: PageProps) {
  const { token } = theme.useToken();
  const { targets, config } = data;
  const ftp = useMemo(() => {
    const m = new Map<string, number>();
    for (const j of config.jobs) {
      if (j.destination.kind !== "ftp") continue;
      const url = ftpUrl({ ...j.destination, remote_dir: "" });
      m.set(url, (m.get(url) ?? 0) + 1);
    }
    return [...m];
  }, [config.jobs]);
  const jobsOn = (id: string) =>
    config.jobs.filter((j) => j.destination.kind === "usb" && j.destination.target_id === id).length;

  return (
    <Panel title="Đích lưu trữ">
      {targets.length === 0 && ftp.length === 0 ? (
        <Typography.Text type="secondary">Chưa cắm ổ đích nào.</Typography.Text>
      ) : (
        <div className="targets">
          {targets.map((d) => {
            const used = d.total_space - d.available_space;
            const pct = d.total_space ? Math.round((used / d.total_space) * 100) : 0;
            const color = pct >= 90 ? token.colorError : pct >= 75 ? token.colorWarning : token.colorPrimary;
            return (
              <div key={d.mount_point} className="target">
                <div className="target-head">
                  <UsbOutlined />
                  <span className="target-name">{d.marker!.label}</span>
                  <span className="target-note">
                    {d.mount_point} · {jobsOn(d.marker!.id)} job
                  </span>
                </div>
                <Progress percent={pct} showInfo={false} size="small" strokeColor={color} />
                <div className="target-foot">
                  Trống {bytes(d.available_space)} / {bytes(d.total_space)}
                </div>
              </div>
            );
          })}
          {ftp.map(([url, n]) => (
            <div key={url} className="target">
              <div className="target-head">
                <CloudServerOutlined />
                <Typography.Text className="target-name mono" ellipsis={{ tooltip: url }}>
                  {url}
                </Typography.Text>
                <span className="target-note">{n} job</span>
              </div>
            </div>
          ))}
        </div>
      )}
    </Panel>
  );
}

type Tone = "ok" | "warn" | "error" | "busy" | "idle";

export default function Dashboard(props: PageProps) {
  const { data, actions, goto } = props;
  const { token } = theme.useToken();
  const { config, history, running, schedule, lastRunByJob, presentTargets } = data;
  const jobs = config.jobs;

  const stats = useMemo(() => {
    const real = history.filter((e) => !e.report.dryRun);
    const week = real.filter((e) => dayjs(e.report.startedAt).isAfter(dayjs().subtract(7, "day")));
    const month = real.filter((e) => dayjs(e.report.startedAt).isAfter(dayjs().subtract(30, "day")));
    const monthOk = month.filter((e) => reportStatus(e.report) === "ok").length;
    const monthErr = month.filter((e) => reportStatus(e.report) === "error").length;
    return {
      usb: jobs.filter((j) => j.destination.kind === "usb").length,
      ftp: jobs.filter((j) => j.destination.kind === "ftp").length,
      weekBytes: week.reduce((s, e) => s + (e.report.bytesCopied || 0), 0),
      weekRuns: week.length,
      rate: monthOk + monthErr ? Math.round((monthOk / (monthOk + monthErr)) * 100) : null,
      monthOk,
      monthTotal: monthOk + monthErr,
      lastOk: real.find((e) => reportStatus(e.report) === "ok"),
    };
  }, [history, jobs]);

  const runningJobs = jobs.filter((j) => running[j.id]);
  const failing = jobs.filter((j) => jobState(j, data) === "error");
  // Chưa backup lần nào, hoặc lần thành công gần nhất đã quá 7 ngày
  const stale = jobs.filter((j) => {
    const e = lastRunByJob.get(j.id);
    return !e || (reportStatus(e.report) === "ok" && dayjs(e.report.finishedAt).isBefore(dayjs().subtract(7, "day")));
  });
  const readyCount = jobs.filter((j) => !running[j.id] && isReady(j, data)).length;
  const next = schedule
    .filter((s) => s.nextRun && !running[s.jobId])
    .sort((a, b) => dayjs(a.nextRun).valueOf() - dayjs(b.nextRun).valueOf())[0];
  const nextJob = next && jobs.find((j) => j.id === next.jobId);

  const names = (list: typeof jobs) =>
    list.length <= 2 ? list.map((j) => j.name).join(", ") : `${list[0].name}, ${list[1].name} và ${list.length - 2} job khác`;

  let tone: Tone, icon: ReactNode, title: string, desc: string;
  if (jobs.length === 0) {
    tone = "idle";
    icon = <SafetyOutlined />;
    title = "Chưa có job backup nào";
    desc = "Thêm job để bắt đầu bảo vệ dữ liệu: chọn thư mục nguồn và ổ USB hoặc máy chủ FTP làm đích.";
  } else if (runningJobs.length > 0) {
    tone = "busy";
    icon = <SyncOutlined spin />;
    title = runningJobs.length === 1 ? "Đang sao lưu" : `Đang sao lưu ${runningJobs.length} job`;
    desc = names(runningJobs);
  } else if (failing.length > 0) {
    tone = "error";
    icon = <WarningFilled />;
    title = `Cần xử lý: ${failing.length} job bị lỗi`;
    desc = `Lượt chạy gần nhất của ${names(failing)} không hoàn tất. Dữ liệu mới có thể chưa được backup.`;
  } else if (stale.length > 0) {
    tone = "warn";
    icon = <ExclamationCircleFilled />;
    title = "Có job chưa được backup gần đây";
    desc = `${names(stale)} chưa backup lần nào hoặc đã quá 7 ngày.`;
  } else {
    tone = "ok";
    icon = <SafetyCertificateFilled />;
    title = "Dữ liệu đang được bảo vệ";
    desc = `Cả ${jobs.length} job đều backup thành công ở lượt gần nhất.`;
  }

  const totals = runningJobs.reduce(
    (s, j) => ({ done: s.done + running[j.id].doneBytes, total: s.total + running[j.id].totalBytes }),
    { done: 0, total: 0 },
  );

  return (
    <div className="page">
      <section className={`hero tone-${tone}`}>
        <div className="hero-shield">{icon}</div>
        <div className="hero-body">
          <div className="hero-eyebrow">Trạng thái bảo vệ</div>
          <h2>{title}</h2>
          <p>{desc}</p>
          {tone === "busy" ? (
            <div className="hero-progress">
              <Progress
                percent={totals.total ? Math.floor((totals.done / totals.total) * 100) : 0}
                status="active"
                size="small"
              />
              <span>
                {bytes(totals.done)} / {bytes(totals.total)}
              </span>
            </div>
          ) : (
            jobs.length > 0 && (
              <div className="hero-facts">
                <div>
                  <span>Thành công gần nhất</span>
                  <b>{stats.lastOk ? <TimeAgo iso={stats.lastOk.report.finishedAt} /> : "Chưa có"}</b>
                </div>
                <div>
                  <span>Lần chạy kế tiếp</span>
                  <b>
                    {next ? (
                      <>
                        <TimeAgo iso={next.nextRun} />
                        {nextJob && <em> · {nextJob.name}</em>}
                      </>
                    ) : (
                      "Theo lần cắm ổ"
                    )}
                  </b>
                </div>
                <div>
                  <span>Ổ đích đang cắm</span>
                  <b>{presentTargets.size}</b>
                </div>
              </div>
            )
          )}
        </div>
        <div className="hero-actions">
          {jobs.length === 0 ? (
            <Button type="primary" size="large" icon={<PlusOutlined />} onClick={actions.create}>
              Thêm job đầu tiên
            </Button>
          ) : (
            <>
              <Button
                type="primary"
                size="large"
                icon={<CloudUploadOutlined />}
                disabled={readyCount === 0}
                onClick={actions.runAll}
              >
                Backup tất cả
              </Button>
              {tone === "error" ? (
                <Button size="large" onClick={() => goto("history")}>
                  Xem lịch sử
                </Button>
              ) : (
                <Button size="large" icon={<PlusOutlined />} onClick={actions.create}>
                  Thêm job
                </Button>
              )}
            </>
          )}
        </div>
      </section>

      <Row gutter={[12, 12]} className="stats">
        <Col xs={12} lg={6}>
          <StatCard
            icon={<UnorderedListOutlined />}
            color={token.colorPrimary}
            title="Job backup"
            value={jobs.length}
            sub={`${stats.usb} ổ đĩa · ${stats.ftp} FTP`}
          />
        </Col>
        <Col xs={12} lg={6}>
          <StatCard
            icon={<CloudUploadOutlined />}
            color={token.purple}
            title="Đã backup 7 ngày qua"
            value={bytes(stats.weekBytes)}
            sub={`${stats.weekRuns} lượt chạy`}
          />
        </Col>
        <Col xs={12} lg={6}>
          <StatCard
            icon={<PieChartOutlined />}
            color={stats.rate !== null && stats.rate < 90 ? token.colorWarning : token.colorSuccess}
            title="Tỉ lệ thành công 30 ngày"
            value={stats.rate === null ? "—" : `${stats.rate}%`}
            sub={stats.monthTotal ? `${stats.monthOk}/${stats.monthTotal} lượt` : "Chưa có lượt chạy"}
          />
        </Col>
        <Col xs={12} lg={6}>
          <StatCard
            icon={<HddOutlined />}
            color={token.cyan}
            title="Ổ đích đang cắm"
            value={presentTargets.size}
            sub={`${data.targets.length ? "Sẵn sàng nhận backup" : "Cắm ổ USB để backup"}`}
          />
        </Col>
      </Row>

      <Row gutter={[16, 16]}>
        <Col xs={24} lg={15} xxl={17}>
          <Panel
            title="Job backup"
            extra={
              jobs.length > 0 && (
                <Button type="link" size="small" onClick={() => goto("jobs")}>
                  Quản lý job
                </Button>
              )
            }
          >
            {jobs.length === 0 ? (
              <Empty description="Chưa có job nào. Cắm ổ USB và đánh dấu làm ổ đích, hoặc thêm job FTP." />
            ) : (
              <div className="job-list">
                {jobs.map((j) => (
                  <JobCard key={j.id} job={j} data={data} actions={actions} compact />
                ))}
              </div>
            )}
          </Panel>
        </Col>
        <Col xs={24} lg={9} xxl={7}>
          <div className="side-stack">
            <ActivityChart history={history} />
            <Targets {...props} />
          </div>
        </Col>
      </Row>
    </div>
  );
}
