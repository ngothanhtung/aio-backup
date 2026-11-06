import { useState, type ReactNode } from "react";
import { App as AntApp } from "antd";
import {
  DashboardOutlined,
  HddOutlined,
  HistoryOutlined,
  RollbackOutlined,
  UnorderedListOutlined,
} from "@ant-design/icons";
import { api, errText, newJob, type Job } from "./api";
import { useAppData, type AppData } from "./useAppData";
import JobDrawer from "./components/JobDrawer";
import { isReady } from "./components/JobCard";
import TitleBar from "./components/TitleBar";
import Dashboard from "./pages/Dashboard";
import Jobs from "./pages/Jobs";
import History from "./pages/History";
import Restore from "./pages/Restore";
import DrivesSettings from "./pages/DrivesSettings";

export type PageKey = "dashboard" | "jobs" | "history" | "restore" | "drives";

type NavItem = { key: PageKey; icon: ReactNode; label: string };

const navGroups: { title: string; items: NavItem[] }[] = [
  {
    title: "Giám sát",
    items: [
      { key: "dashboard", icon: <DashboardOutlined />, label: "Tổng quan" },
      { key: "history", icon: <HistoryOutlined />, label: "Lịch sử" },
    ],
  },
  {
    title: "Sao lưu",
    items: [
      { key: "jobs", icon: <UnorderedListOutlined />, label: "Job backup" },
      { key: "restore", icon: <RollbackOutlined />, label: "Khôi phục" },
    ],
  },
  {
    title: "Hệ thống",
    items: [{ key: "drives", icon: <HddOutlined />, label: "Ổ đĩa & Cài đặt" }],
  },
];

/** Thao tác trên job, dùng chung cho các trang. */
export interface JobActions {
  run: (job: Job, dryRun: boolean) => void;
  /** Chạy mọi job đang sẵn sàng (đích có mặt, chưa chạy). */
  runAll: () => void;
  cancel: (job: Job) => void;
  edit: (job: Job) => void;
  remove: (job: Job) => void;
  create: () => void;
  restore: (job: Job) => void;
}

export interface PageProps {
  data: AppData;
  actions: JobActions;
  goto: (p: PageKey) => void;
}

export default function App() {
  const { message, notification, modal } = AntApp.useApp();
  const data = useAppData((e) => {
    // Chỉ báo khi có lỗi; kết quả thành công đã hiện ngay trên bảng
    if (e.error) {
      notification.error({ title: `${e.jobName}: không chạy được`, description: e.error });
    } else if (e.report && (e.report.failed.length > 0 || e.report.targetLost)) {
      notification.warning({
        title: `${e.jobName}: kết thúc có lỗi`,
        description: e.report.targetLost
          ? "Mất kết nối tới đích giữa chừng"
          : `${e.report.failed.length} file không backup được`,
      });
    }
  });
  const [page, setPage] = useState<PageKey>("dashboard");
  const runningCount = Object.keys(data.running).length;
  const [editing, setEditing] = useState<Job | null>(null);
  const [restoreJob, setRestoreJob] = useState<string | undefined>();

  const run = async (job: Job, dryRun: boolean) => {
    data.markRunning(job.id, dryRun);
    try {
      await api.startJob(job.id, dryRun);
    } catch (e) {
      data.clearRunning(job.id);
      message.error(errText(e));
    }
  };

  const actions: JobActions = {
    run,
    runAll: () => {
      const ready = data.config.jobs.filter((j) => !data.running[j.id] && isReady(j, data));
      ready.forEach((j) => run(j, false));
      if (ready.length) message.info(`Đã bắt đầu ${ready.length} job`);
    },
    cancel: (job) => api.cancelJob(job.id),
    edit: (job) => setEditing(structuredClone(job)),
    remove: (job) =>
      modal.confirm({
        title: `Xoá job "${job.name}"?`,
        content: "Dữ liệu đã backup ở đích vẫn được giữ nguyên.",
        okText: "Xoá",
        okButtonProps: { danger: true },
        cancelText: "Huỷ",
        onOk: async () => {
          data.setConfig(await api.deleteJob(job.id));
          data.refreshSchedule();
        },
      }),
    create: () => setEditing(newJob(data.config, data.targets[0]?.marker?.id)),
    restore: (job) => {
      setRestoreJob(job.id);
      setPage("restore");
    },
  };

  const props: PageProps = { data, actions, goto: setPage };

  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="brand" data-tauri-drag-region>
          <img src="/icon.png" alt="" />
          <div className="brand-text">
            <span>AIO Backup</span>
            <small>Sao lưu USB & FTP</small>
          </div>
        </div>
        <nav className="nav">
          {navGroups.map((g) => (
            <div key={g.title} className="nav-group">
              <div className="nav-group-title">{g.title}</div>
              {g.items.map((item) => (
                <button
                  key={item.key}
                  className={page === item.key ? "nav-item active" : "nav-item"}
                  title={item.label}
                  onClick={() => setPage(item.key)}
                >
                  {item.icon}
                  <span className="nav-label">{item.label}</span>
                  {item.key === "jobs" && runningCount > 0 && <span className="nav-badge">{runningCount}</span>}
                </button>
              ))}
            </div>
          ))}
        </nav>
        <div className="sidebar-foot">
          <span className={runningCount ? "status-dot busy" : "status-dot"} />
          <div className="sidebar-foot-text">
            <span>{runningCount ? `Đang chạy ${runningCount} job` : "Sẵn sàng"}</span>
            <small>
              {data.presentTargets.size} ổ đích · {data.config.jobs.length} job
            </small>
          </div>
        </div>
      </aside>
      <div className="main">
        <TitleBar />
        <main className="content">
          {page === "dashboard" && <Dashboard {...props} />}
          {page === "jobs" && <Jobs {...props} />}
          {page === "history" && <History {...props} />}
          {page === "restore" && <Restore {...props} initialJobId={restoreJob} />}
          {page === "drives" && <DrivesSettings {...props} />}
        </main>
      </div>

      <JobDrawer
        job={editing}
        config={data.config}
        drives={data.drives}
        onDrivesChanged={data.refreshDrives}
        onClose={() => setEditing(null)}
        onSaved={(cfg, warning) => {
          data.setConfig(cfg);
          data.refreshSchedule();
          setEditing(null);
          message.success("Đã lưu job");
          if (warning) notification.warning({ title: "Mật khẩu FTP", description: warning });
        }}
      />
    </div>
  );
}
