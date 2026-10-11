import { useEffect, useState } from "react";
import {
  Alert,
  Button,
  Checkbox,
  Col,
  Divider,
  Drawer,
  Form,
  Input,
  InputNumber,
  Row,
  Segmented,
  Select,
  Space,
  Switch,
  Typography,
} from "antd";
import { CloudServerOutlined, FolderOpenOutlined, UsbOutlined } from "@ant-design/icons";
import { open } from "@tauri-apps/plugin-dialog";
import { api, errText, type Config, type DriveInfo, type FtpTarget, type Job } from "../api";

interface FormValues {
  name: string;
  source: string;
  kind: "usb" | "ftp";
  target_id?: string;
  host: string;
  port: number;
  username: string;
  password?: string;
  remote_dir: string;
  secure: boolean;
  accept_invalid_certs: boolean;
  dest_subdir: string;
  excludes: string[];
  auto_run: boolean;
  interval_minutes: number;
  versioning: boolean;
  keep_versions_days: number;
  mirror_deletes: boolean;
  verify_hash: boolean;
}

function toForm(job: Job): FormValues {
  const d = job.destination;
  const ftp = d.kind === "ftp" ? d : undefined;
  return {
    name: job.name,
    source: job.source,
    kind: d.kind,
    target_id: d.kind === "usb" ? d.target_id || undefined : undefined,
    host: ftp?.host ?? "",
    port: ftp?.port ?? 21,
    username: ftp?.username ?? "",
    password: "",
    remote_dir: ftp?.remote_dir ?? "/backup",
    secure: ftp?.secure ?? true,
    accept_invalid_certs: ftp?.accept_invalid_certs ?? false,
    dest_subdir: job.dest_subdir,
    excludes: job.excludes,
    auto_run: job.auto_run,
    interval_minutes: job.interval_minutes ?? 0,
    versioning: job.versioning,
    keep_versions_days: job.keep_versions_days ?? 0,
    mirror_deletes: job.mirror_deletes,
    verify_hash: job.verify_hash,
  };
}

function ftpOf(v: FormValues): FtpTarget {
  return {
    host: (v.host ?? "").trim(),
    port: v.port || 21,
    username: (v.username ?? "").trim(),
    remote_dir: (v.remote_dir ?? "").trim(),
    secure: !!v.secure,
    accept_invalid_certs: !!v.secure && !!v.accept_invalid_certs,
  };
}

function toJob(base: Job, v: FormValues): Job {
  return {
    ...base,
    name: v.name.trim(),
    source: v.source.trim(),
    destination: v.kind === "usb" ? { kind: "usb", target_id: v.target_id! } : { kind: "ftp", ...ftpOf(v) },
    dest_subdir: (v.dest_subdir ?? "").trim(),
    excludes: v.excludes ?? [],
    auto_run: v.kind === "usb" ? !!v.auto_run : true,
    interval_minutes: v.interval_minutes > 0 ? v.interval_minutes : null,
    versioning: !!v.versioning,
    keep_versions_days: v.versioning && v.keep_versions_days > 0 ? v.keep_versions_days : null,
    mirror_deletes: !!v.mirror_deletes,
    verify_hash: v.kind === "usb" && !!v.verify_hash,
  };
}

export default function JobDrawer(props: {
  job: Job | null;
  config: Config;
  targets: DriveInfo[];
  onClose: () => void;
  onSaved: (cfg: Config, warning: string | null) => void;
}) {
  const { job } = props;
  const [form] = Form.useForm<FormValues>();
  const kind = Form.useWatch("kind", form);
  const secure = Form.useWatch("secure", form);
  const versioning = Form.useWatch("versioning", form);
  const mirror = Form.useWatch("mirror_deletes", form);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [testMsg, setTestMsg] = useState<{ ok: boolean; text: string } | null>(null);

  const isNew = !!job && !props.config.jobs.some((j) => j.id === job.id);

  useEffect(() => {
    if (job) {
      form.resetFields();
      form.setFieldsValue(toForm(job));
      setError(null);
      setTestMsg(null);
    }
  }, [job, form]);

  if (!job) return null;

  const usbOptions = props.targets.map((t) => ({
    value: t.marker!.id,
    label: `${t.marker!.label} — ${t.mount_point}`,
  }));
  const currentUsb = job.destination.kind === "usb" ? job.destination.target_id : "";
  if (currentUsb && !usbOptions.some((o) => o.value === currentUsb)) {
    usbOptions.unshift({ value: currentUsb, label: "(ổ hiện chưa cắm)" });
  }

  const pickSource = async () => {
    const dir = await open({ directory: true, multiple: false, title: "Chọn thư mục cần backup" });
    if (typeof dir === "string") {
      form.setFieldValue("source", dir);
      if (!form.getFieldValue("name")) {
        form.setFieldValue("name", dir.split(/[\\/]/).filter(Boolean).pop() ?? "");
      }
      form.validateFields(["source", "name"]).catch(() => {});
    }
  };

  const testConnection = async () => {
    setTesting(true);
    setTestMsg(null);
    try {
      const v = form.getFieldsValue(true) as FormValues;
      const msg = await api.testFtp(ftpOf(v), v.password ?? "", isNew ? undefined : job.id);
      setTestMsg({ ok: true, text: msg });
    } catch (e) {
      setTestMsg({ ok: false, text: errText(e) });
    } finally {
      setTesting(false);
    }
  };

  const submit = async (v: FormValues) => {
    setSaving(true);
    setError(null);
    try {
      const res = await api.saveJob(toJob(job, v), v.kind === "ftp" ? v.password : undefined);
      props.onSaved(res.config, res.warning);
    } catch (e) {
      setError(errText(e));
    } finally {
      setSaving(false);
    }
  };

  const onKindChange = (k: "usb" | "ftp") => {
    setTestMsg(null);
    const interval = form.getFieldValue("interval_minutes") as number;
    // FTP không có sự kiện "cắm ổ" -> gợi ý chạy mỗi giờ
    if (k === "ftp" && !interval) form.setFieldValue("interval_minutes", 60);
    if (k === "usb" && !form.getFieldValue("target_id") && usbOptions[0]) {
      form.setFieldValue("target_id", usbOptions[0].value);
    }
  };

  return (
    <Drawer
      open
      size={600}
      title={isNew ? "Job mới" : `Sửa job: ${job.name}`}
      onClose={props.onClose}
      destroyOnHidden
      footer={
        <Space style={{ justifyContent: "flex-end", width: "100%" }}>
          <Button onClick={props.onClose}>Huỷ</Button>
          <Button type="primary" loading={saving} onClick={() => form.submit()}>
            Lưu
          </Button>
        </Space>
      }
    >
      <Form form={form} layout="vertical" onFinish={submit} requiredMark={false}>
        <Form.Item label="Thư mục nguồn" required>
          <Space.Compact style={{ width: "100%" }}>
            <Form.Item name="source" noStyle rules={[{ required: true, message: "Chọn thư mục cần backup" }]}>
              <Input placeholder="Thư mục trên HDD/SSD" />
            </Form.Item>
            <Button icon={<FolderOpenOutlined />} onClick={pickSource}>
              Chọn…
            </Button>
          </Space.Compact>
        </Form.Item>

        <Form.Item name="name" label="Tên job" rules={[{ required: true, message: "Đặt tên cho job" }]}>
          <Input placeholder="VD: Tài liệu giảng dạy" />
        </Form.Item>

        <Form.Item name="kind" label="Backup tới">
          <Segmented<"usb" | "ftp">
            onChange={onKindChange}
            options={[
              { value: "usb", label: "Ổ USB", icon: <UsbOutlined /> },
              { value: "ftp", label: "FTP / FTPS", icon: <CloudServerOutlined /> },
            ]}
          />
        </Form.Item>

        {kind === "usb" && (
          <Form.Item
            name="target_id"
            label="Ổ đích"
            rules={[{ required: true, message: "Chọn ổ đích" }]}
            extra={
              usbOptions.length === 0
                ? "Chưa có ổ đích. Cắm ổ USB rồi đánh dấu nó ở trang “Ổ đĩa & Cài đặt”."
                : undefined
            }
          >
            <Select options={usbOptions} placeholder="Chọn ổ USB đã đánh dấu" />
          </Form.Item>
        )}

        {kind === "ftp" && (
          <>
            <Row gutter={12}>
              <Col flex="auto">
                <Form.Item name="host" label="Máy chủ" rules={[{ required: true, message: "Nhập host" }]}>
                  <Input placeholder="192.168.1.10 hoặc nas.example.com" onChange={() => setTestMsg(null)} />
                </Form.Item>
              </Col>
              <Col flex="120px">
                <Form.Item name="port" label="Cổng">
                  <InputNumber min={1} max={65535} style={{ width: "100%" }} />
                </Form.Item>
              </Col>
            </Row>
            <Row gutter={12}>
              <Col span={12}>
                <Form.Item
                  name="username"
                  label="Tên đăng nhập"
                  rules={[{ required: true, message: "Nhập tên đăng nhập" }]}
                >
                  <Input autoComplete="off" />
                </Form.Item>
              </Col>
              <Col span={12}>
                <Form.Item
                  name="password"
                  label="Mật khẩu"
                  rules={isNew ? [{ required: true, message: "Nhập mật khẩu" }] : []}
                >
                  <Input.Password
                    autoComplete="new-password"
                    placeholder={isNew ? "" : "Để trống = giữ mật khẩu cũ"}
                  />
                </Form.Item>
              </Col>
            </Row>
            <Form.Item name="remote_dir" label="Thư mục trên server">
              <Input placeholder="/backup" />
            </Form.Item>
            <Space orientation="vertical" size={4} style={{ width: "100%", marginBottom: 12 }}>
              <Form.Item name="secure" valuePropName="checked" noStyle>
                <Checkbox>Mã hoá FTPS (khuyên dùng)</Checkbox>
              </Form.Item>
              {secure && (
                <Form.Item name="accept_invalid_certs" valuePropName="checked" noStyle>
                  <Checkbox>Chấp nhận chứng chỉ tự ký (chỉ cho NAS trong mạng nội bộ)</Checkbox>
                </Form.Item>
              )}
            </Space>
            {!secure && (
              <Alert
                type="warning"
                showIcon
                style={{ marginBottom: 12 }}
                title="FTP thường gửi mật khẩu và dữ liệu không mã hoá. Chỉ nên dùng trong mạng nội bộ."
              />
            )}
            <Space wrap style={{ marginBottom: 8 }}>
              <Button onClick={testConnection} loading={testing}>
                Kiểm tra kết nối
              </Button>
            </Space>
            {testMsg && (
              <Alert
                type={testMsg.ok ? "success" : "error"}
                showIcon
                title={testMsg.text}
                style={{ marginBottom: 12 }}
              />
            )}
          </>
        )}

        <Form.Item name="dest_subdir" label="Thư mục con ở đích" extra="Để trống = dùng tên job">
          <Input />
        </Form.Item>

        <Form.Item
          name="excludes"
          label="Loại trừ"
          extra="Gõ pattern rồi Enter, vd: node_modules, *.tmp, Cache/**"
        >
          <Select mode="tags" tokenSeparators={[","]} open={false} suffixIcon={null} />
        </Form.Item>

        <Divider titlePlacement="start" plain>
          Lịch chạy
        </Divider>
        {kind === "usb" && (
          <Form.Item name="auto_run" valuePropName="checked" label="Tự chạy khi cắm ổ đích">
            <Switch />
          </Form.Item>
        )}
        <Form.Item
          name="interval_minutes"
          label="Chạy định kỳ"
          extra={kind === "usb" ? "Chỉ khi ổ đang cắm. 0 = tắt." : "0 = chỉ chạy thủ công."}
        >
          <InputNumber min={0} max={10080} suffix="phút" style={{ width: 160 }} />
        </Form.Item>

        <Divider titlePlacement="start" plain>
          Phiên bản & xoá
        </Divider>
        <Form.Item name="versioning" valuePropName="checked" label="Giữ bản cũ trong .versions khi file bị ghi đè/xoá">
          <Switch />
        </Form.Item>
        {versioning && (
          <Form.Item name="keep_versions_days" label="Tự xoá bản cũ sau" extra="0 = giữ mãi">
            <InputNumber min={0} max={3650} suffix="ngày" style={{ width: 160 }} />
          </Form.Item>
        )}
        <Form.Item name="mirror_deletes" valuePropName="checked" label="Mirror: xoá ở đích file đã xoá ở nguồn">
          <Switch />
        </Form.Item>
        {mirror && !versioning && (
          <Alert
            type="warning"
            showIcon
            style={{ marginBottom: 12 }}
            title="Mirror mà không giữ bản cũ: xoá nhầm ở nguồn thì bản backup cũng mất."
          />
        )}
        {kind === "usb" && (
          <Form.Item
            name="verify_hash"
            valuePropName="checked"
            label="So nội dung (blake3) khi chỉ khác thời gian sửa"
          >
            <Switch />
          </Form.Item>
        )}

        {error && <Alert type="error" showIcon title={error} />}
        <Typography.Text type="secondary" style={{ fontSize: 12 }}>
          Mật khẩu FTP được lưu trong kho mật khẩu của hệ điều hành.
        </Typography.Text>
      </Form>
    </Drawer>
  );
}
