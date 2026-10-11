import { useState } from "react";
import { App as AntApp, Layout, Menu, type MenuProps } from "antd";
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
import Dashboard from "./pages/Dashboard";
import Jobs from "./pages/Jobs";
import History from "./pages/History";
import Restore from "./pages/Restore";
import DrivesSettings from "./pages/DrivesSettings";

export type PageKey = "dashboard" | "jobs" | "history" | "restore" | "drives";

const menuItems: MenuProps["items"] = [
  { key: "dashboard", icon: <DashboardOutlined />, label: "Tổng quan" },
  { key: "jobs", icon: <UnorderedListOutlined />, label: "Job backup" },
  { key: "history", icon: <HistoryOutlined />, label: "Lịch sử" },
  { key: "restore", icon: <RollbackOutlined />, label: "Khôi phục" },
  { key: "drives", icon: <HddOutlined />, label: "Ổ đĩa & Cài đặt" },
];

/** Thao tác trên job, dùng chung cho các trang. */
export interface JobActions {
  run: (job: Job, dryRun: boolean) => void;
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
  const [collapsed, setCollapsed] = useState(false);
  const [editing, setEditing] = useState<Job | null>(null);
  const [restoreJob, setRestoreJob] = useState<string | undefined>();

  const actions: JobActions = {
    run: async (job, dryRun) => {
      data.markRunning(job.id, dryRun);
      try {
        await api.startJob(job.id, dryRun);
      } catch (e) {
        data.clearRunning(job.id);
        message.error(errText(e));
      }
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
    <Layout className="app-layout">
      <Layout.Sider
        className="app-sider"
        theme="light"
        width={208}
        collapsible
        collapsed={collapsed}
        onCollapse={setCollapsed}
        breakpoint="md"
        style={{ position: "sticky", top: 0, height: "100vh", overflow: "auto" }}
      >
        <div className="brand">
          <img src="/icon.png" alt="" />
          {!collapsed && <span>AIO Backup</span>}
        </div>
        <Menu
          mode="inline"
          selectedKeys={[page]}
          items={menuItems}
          onClick={(e) => setPage(e.key as PageKey)}
          style={{ borderInlineEnd: "none" }}
        />
      </Layout.Sider>
      <Layout>
        <Layout.Content>
          {page === "dashboard" && <Dashboard {...props} />}
          {page === "jobs" && <Jobs {...props} />}
          {page === "history" && <History {...props} />}
          {page === "restore" && <Restore {...props} initialJobId={restoreJob} />}
          {page === "drives" && <DrivesSettings {...props} />}
        </Layout.Content>
      </Layout>

      <JobDrawer
        job={editing}
        config={data.config}
        targets={data.targets}
        onClose={() => setEditing(null)}
        onSaved={(cfg, warning) => {
          data.setConfig(cfg);
          data.refreshSchedule();
          setEditing(null);
          message.success("Đã lưu job");
          if (warning) notification.warning({ title: "Mật khẩu FTP", description: warning });
        }}
      />
    </Layout>
  );
}
