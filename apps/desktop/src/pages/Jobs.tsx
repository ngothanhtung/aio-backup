import { useState } from "react";
import { Button, Empty, Segmented } from "antd";
import { PlusOutlined } from "@ant-design/icons";
import type { PageProps } from "../App";
import { PageHead } from "../components/common";
import JobCard, { jobState } from "../components/JobCard";

type Filter = "all" | "usb" | "ftp" | "issue";

export default function Jobs({ data, actions }: PageProps) {
  const jobs = data.config.jobs;
  const [filter, setFilter] = useState<Filter>("all");

  const isIssue = (s: ReturnType<typeof jobState>) => s === "error" || s === "never";
  const issues = jobs.filter((j) => isIssue(jobState(j, data))).length;
  const shown = jobs.filter((j) => {
    if (filter === "usb" || filter === "ftp") return j.destination.kind === filter;
    if (filter === "issue") return isIssue(jobState(j, data));
    return true;
  });

  return (
    <div className="page">
      <PageHead
        title="Job backup"
        sub="Mỗi job sao lưu một thư mục nguồn sang ổ USB hoặc máy chủ FTP, chạy theo lịch hoặc khi cắm ổ."
        extra={
          <Button type="primary" icon={<PlusOutlined />} onClick={actions.create}>
            Thêm job
          </Button>
        }
      />

      {jobs.length > 0 && (
        <Segmented<Filter>
          className="filter-bar"
          value={filter}
          onChange={setFilter}
          options={[
            { value: "all", label: `Tất cả (${jobs.length})` },
            { value: "usb", label: "Ổ đĩa" },
            { value: "ftp", label: "FTP" },
            { value: "issue", label: `Cần chú ý${issues ? ` (${issues})` : ""}` },
          ]}
        />
      )}

      {shown.length === 0 ? (
        <div className="panel">
          <Empty description={jobs.length ? "Không có job nào khớp bộ lọc" : "Chưa có job nào"}>
            {!jobs.length && (
              <Button type="primary" onClick={actions.create}>
                Thêm job đầu tiên
              </Button>
            )}
          </Empty>
        </div>
      ) : (
        <div className="job-grid">
          {shown.map((j) => (
            <JobCard key={j.id} job={j} data={data} actions={actions} />
          ))}
        </div>
      )}
    </div>
  );
}
