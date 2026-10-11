import { useEffect, useState } from "react";
import {
  App as AntApp,
  Button,
  Card,
  Form,
  Input,
  InputNumber,
  Modal,
  Progress,
  Select,
  Space,
  Switch,
  Table,
  Tag,
  Typography,
} from "antd";
import { ReloadOutlined, UsbOutlined } from "@ant-design/icons";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { api, bytes, errText, type DriveInfo } from "../api";
import type { PageProps } from "../App";

export default function DrivesSettings({ data }: PageProps) {
  const { message } = AntApp.useApp();
  const { drives, refreshDrives, config, setConfig } = data;
  const [showAll, setShowAll] = useState(false);
  const [marking, setMarking] = useState<DriveInfo | null>(null);
  const [label, setLabel] = useState("");

  const [autostart, setAutostart] = useState(false);
  const [form] = Form.useForm<{ poll: number; excludes: string[]; keepDays: number }>();
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    isEnabled().then(setAutostart).catch(() => {});
  }, []);
  useEffect(() => {
    form.setFieldsValue({
      poll: config.poll_interval_secs,
      excludes: config.default_excludes,
      keepDays: config.default_keep_days,
    });
  }, [config, form]);

  // Mặc định chỉ hiện ổ rời và ổ đã đánh dấu, tránh rối với phân vùng hệ thống
  const visible = showAll ? drives : drives.filter((d) => d.removable || d.marker);

  const mark = async () => {
    if (!marking) return;
    try {
      await api.markTarget(marking.mount_point, label.trim() || marking.name || "Ổ backup");
      message.success("Đã đánh dấu ổ đích");
      setMarking(null);
      refreshDrives();
    } catch (e) {
      message.error(errText(e));
    }
  };

  const toggleAutostart = async (on: boolean) => {
    try {
      if (on) await enable();
      else await disable();
      setAutostart(await isEnabled());
    } catch (e) {
      message.error(errText(e));
    }
  };

  const saveSettings = async () => {
    const v = await form.validateFields();
    setSaving(true);
    try {
      setConfig(
        await api.saveSettings({
          pollIntervalSecs: v.poll,
          defaultExcludes: v.excludes ?? [],
          defaultKeepDays: v.keepDays ?? 0,
        }),
      );
      message.success("Đã lưu cài đặt");
    } catch (e) {
      message.error(errText(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="page">
      <div className="page-head">
        <h1>Ổ đĩa & Cài đặt</h1>
      </div>

      <Card
        title="Ổ đĩa"
        extra={
          <Space>
            <Switch size="small" checked={showAll} onChange={setShowAll} />
            <Typography.Text type="secondary">Hiện cả ổ hệ thống</Typography.Text>
            <Button size="small" icon={<ReloadOutlined />} onClick={refreshDrives} />
          </Space>
        }
        styles={{ body: { padding: 0 } }}
      >
        <Table<DriveInfo>
          rowKey="mount_point"
          dataSource={visible}
          pagination={false}
          locale={{ emptyText: "Chưa thấy ổ USB nào. Cắm ổ vào, danh sách sẽ tự cập nhật." }}
          columns={[
            {
              title: "Ổ",
              render: (_, d) => (
                <Space orientation="vertical" size={0}>
                  <Space size={6}>
                    <Typography.Text strong>{d.marker?.label ?? (d.name || d.mount_point)}</Typography.Text>
                    {d.marker && (
                      <Tag color="blue" icon={<UsbOutlined />}>
                        Ổ đích
                      </Tag>
                    )}
                    {d.removable && !d.marker && <Tag>Ổ rời</Tag>}
                  </Space>
                  <Typography.Text type="secondary" className="mono">
                    {d.mount_point} · {d.file_system}
                  </Typography.Text>
                </Space>
              ),
            },
            {
              title: "Dung lượng",
              width: 240,
              render: (_, d) => {
                const used = d.total_space ? 1 - d.available_space / d.total_space : 0;
                return (
                  <div>
                    <Progress
                      percent={Math.round(used * 100)}
                      size="small"
                      status={used > 0.9 ? "exception" : "normal"}
                      showInfo={false}
                    />
                    <Typography.Text type="secondary" style={{ fontSize: 12 }}>
                      Trống {bytes(d.available_space)} / {bytes(d.total_space)}
                    </Typography.Text>
                  </div>
                );
              },
            },
            {
              title: "",
              width: 170,
              align: "right",
              render: (_, d) =>
                d.marker ? (
                  <Typography.Text type="secondary" className="mono" style={{ fontSize: 11 }}>
                    {d.marker.id.slice(0, 8)}
                  </Typography.Text>
                ) : (
                  <Button
                    size="small"
                    onClick={() => {
                      setLabel(d.name);
                      setMarking(d);
                    }}
                  >
                    Đánh dấu làm ổ đích
                  </Button>
                ),
            },
          ]}
        />
      </Card>

      <Card title="Cài đặt" style={{ marginTop: 16 }}>
        <Form form={form} layout="vertical" style={{ maxWidth: 520 }} requiredMark={false}>
          <Form.Item label="Khởi động cùng máy" extra="Chạy ẩn dưới khay hệ thống khi đăng nhập.">
            <Switch checked={autostart} onChange={toggleAutostart} />
          </Form.Item>
          <Form.Item
            name="poll"
            label="Chu kỳ kiểm tra ổ đĩa và lịch"
            rules={[{ required: true }]}
            extra="Áp dụng từ vòng kiểm tra kế tiếp."
          >
            <InputNumber min={2} max={3600} suffix="giây" style={{ width: 160 }} />
          </Form.Item>
          <Form.Item name="excludes" label="Loại trừ mặc định cho job mới">
            <Select mode="tags" tokenSeparators={[","]} open={false} suffixIcon={null} />
          </Form.Item>
          <Form.Item name="keepDays" label="Giữ bản cũ mặc định" extra="0 = giữ mãi">
            <InputNumber min={0} max={3650} suffix="ngày" style={{ width: 160 }} />
          </Form.Item>
          <Button type="primary" loading={saving} onClick={saveSettings}>
            Lưu cài đặt
          </Button>
        </Form>
      </Card>

      <Modal
        open={!!marking}
        title="Đánh dấu làm ổ đích"
        okText="Đánh dấu"
        cancelText="Huỷ"
        onOk={mark}
        onCancel={() => setMarking(null)}
      >
        <Typography.Paragraph type="secondary">
          App ghi file <code>.backup-id</code> vào gốc ổ <b>{marking?.mount_point}</b> để nhận ra ổ này mỗi khi
          cắm vào, kể cả khi ký tự ổ hay đường dẫn mount thay đổi.
        </Typography.Paragraph>
        <Input
          autoFocus
          placeholder="Tên ổ, vd: Samsung T7"
          value={label}
          onChange={(e) => setLabel(e.target.value)}
          onPressEnter={mark}
        />
      </Modal>
    </div>
  );
}
