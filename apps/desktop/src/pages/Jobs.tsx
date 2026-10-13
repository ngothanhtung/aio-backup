import { Button, Dropdown, Empty, Space, Table, Tag, Tooltip, Typography } from "antd";
import {
  CaretRightOutlined,
  ClockCircleOutlined,
  DeleteOutlined,
  EditOutlined,
  EyeOutlined,
  MoreOutlined,
  PlusOutlined,
  RollbackOutlined,
  StopOutlined,
  ThunderboltOutlined,
} from "@ant-design/icons";
import { bytes, type Job } from "../api";
import type { PageProps } from "../App";
import {
  Counts,
  DestLabel,
  everyLabel,
  RunProgress,
  scheduleLabel,
  StatusTag,
  TimeAgo,
} from "../components/common";

export default function Jobs({ data, actions }: PageProps) {
  const { config, running, results, lastRunByJob, presentTargets } = data;

  return (
    <div className="page">
      <div className="page-head">
        <h1>Job backup</h1>
        <Button type="primary" icon={<PlusOutlined />} onClick={actions.create}>
          Thêm job
        </Button>
      </div>

      <Table<Job>
        rowKey="id"
        dataSource={config.jobs}
        pagination={false}
        tableLayout="fixed"
        locale={{ emptyText: <Empty description="Chưa có job nào" /> }}
        columns={[
          {
            title: "Job",
            render: (_, j) => (
              <div className="cell-stack">
                <Typography.Text strong>{j.name}</Typography.Text>
                <Typography.Text type="secondary" className="mono" ellipsis={{ tooltip: j.source }} style={{ display: "block" }}>
                  {j.source}
                </Typography.Text>
                <DestLabel
                  job={j}
                  drive={j.destination.kind === "usb" ? presentTargets.get(j.destination.target_id) : undefined}
                />
              </div>
            ),
          },
          {
            title: "Chạy khi",
            width: 150,
            render: (_, j) => (
              <Space orientation="vertical" size={4}>
                {j.destination.kind === "usb" && j.auto_run && (
                  <Tag icon={<ThunderboltOutlined />}>Cắm ổ</Tag>
                )}
                {j.interval_minutes ? (
                  <Tag icon={<ClockCircleOutlined />}>{everyLabel(j.interval_minutes)}</Tag>
                ) : null}
                {j.schedule && (
                  <Tag icon={<ClockCircleOutlined />} style={{ whiteSpace: "normal" }}>
                    {scheduleLabel(j.schedule)}
                  </Tag>
                )}
                {!j.interval_minutes && !j.schedule && !(j.destination.kind === "usb" && j.auto_run) && (
                  <Typography.Text type="secondary">Thủ công</Typography.Text>
                )}
              </Space>
            ),
          },
          {
            title: "Trạng thái",
            width: 240,
            render: (_, j) => {
              if (running[j.id]) return <RunProgress run={running[j.id]} />;
              const last = lastRunByJob.get(j.id);
              const preview = results[j.id]?.report?.dryRun ? results[j.id]!.report! : null;
              return (
                <Space orientation="vertical" size={2}>
                  {last ? (
                    <Space size={6} wrap>
                      <StatusTag report={last.report} />
                      <Counts report={last.report} />
                      <Typography.Text type="secondary">
                        <TimeAgo iso={last.report.finishedAt} />
                      </Typography.Text>
                    </Space>
                  ) : (
                    <Typography.Text type="secondary">Chưa chạy</Typography.Text>
                  )}
                  {preview && (
                    <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                      Xem trước: sẽ +{preview.created} ~{preview.updated} -{preview.deleted} ·{" "}
                      {bytes(preview.bytesCopied)}
                    </Typography.Text>
                  )}
                </Space>
              );
            },
          },
          {
            title: "",
            width: 190,
            align: "right",
            render: (_, j) => {
              const ready = j.destination.kind === "ftp" || presentTargets.has(j.destination.target_id);
              if (running[j.id]) {
                return (
                  <Button icon={<StopOutlined />} onClick={() => actions.cancel(j)}>
                    Huỷ
                  </Button>
                );
              }
              return (
                <Space size={4}>
                  <Tooltip title={ready ? "" : "Ổ đích chưa cắm"}>
                    <Button
                      type="primary"
                      icon={<CaretRightOutlined />}
                      disabled={!ready}
                      onClick={() => actions.run(j, false)}
                    >
                      Chạy
                    </Button>
                  </Tooltip>
                  <Tooltip title="Xem trước: liệt kê thay đổi, không ghi gì">
                    <Button icon={<EyeOutlined />} disabled={!ready} onClick={() => actions.run(j, true)} />
                  </Tooltip>
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
                        if (key === "edit") actions.edit(j);
                        if (key === "restore") actions.restore(j);
                        if (key === "delete") actions.remove(j);
                      },
                    }}
                  >
                    <Button icon={<MoreOutlined />} />
                  </Dropdown>
                </Space>
              );
            },
          },
        ]}
      />
    </div>
  );
}
