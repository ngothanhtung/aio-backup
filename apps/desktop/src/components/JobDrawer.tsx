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
  TimePicker,
  Typography,
} from "antd";
import { CloudServerOutlined, FolderOpenOutlined, UsbOutlined } from "@ant-design/icons";
import { open } from "@tauri-apps/plugin-dialog";
import dayjs, { type Dayjs } from "dayjs";
import {
  api,
  bytes,
  errText,
  type Config,
  type DriveInfo,
  type FtpTarget,
  type Job,
  type Schedule,
} from "../api";
import { WEEKDAYS } from "./common";

type Repeat = "none" | "interval" | Schedule["every"];

/** Giá trị tạm của ô "Ổ đích" cho ổ chưa đánh dấu: tiền tố + mount point. */
const UNMARKED = "path:";

const toTime = (s?: string): Dayjs | null => {
  if (!s) return null;
  const [h, m] = s.split(":").map(Number);
  return dayjs().hour(h).minute(m).second(0);
};

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
  repeat: Repeat;
  interval_minutes: number;
  at: string;
  weekdays: number[];
  month_day: number;
  versioning: boolean;
  keep_versions_days: number;
  mirror_deletes: boolean;
  verify_hash: boolean;
}

function scheduleOf(v: FormValues): Schedule | null {
  switch (v.repeat) {
    case "daily":
      return { every: "daily", at: v.at };
    case "weekly":
      return { every: "weekly", at: v.at, weekdays: v.weekdays };
    case "monthly":
      return { every: "monthly", at: v.at, day: v.month_day };
    default:
      return null;
  }
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
    repeat: job.schedule?.every ?? (job.interval_minutes ? "interval" : "none"),
    interval_minutes: job.interval_minutes ?? 60,
    at: job.schedule?.at ?? "02:00",
    weekdays: job.schedule?.every === "weekly" ? job.schedule.weekdays : [1],
    month_day: job.schedule?.every === "monthly" ? job.schedule.day : 1,
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
    interval_minutes: v.repeat === "interval" && v.interval_minutes > 0 ? v.interval_minutes : null,
    schedule: scheduleOf(v),
    versioning: !!v.versioning,
    keep_versions_days: v.versioning && v.keep_versions_days > 0 ? v.keep_versions_days : null,
    mirror_deletes: !!v.mirror_deletes,
    verify_hash: v.kind === "usb" && !!v.verify_hash,
  };
}

export default function JobDrawer(props: {
  job: Job | null;
  config: Config;
  drives: DriveInfo[];
  onDrivesChanged: () => void;
  onClose: () => void;
  onSaved: (cfg: Config, warning: string | null) => void;
}) {
  const { job } = props;
  const [form] = Form.useForm<FormValues>();
  const kind = Form.useWatch("kind", form);
  const secure = Form.useWatch("secure", form);
  const versioning = Form.useWatch("versioning", form);
  const mirror = Form.useWatch("mirror_deletes", form);
  const repeat = Form.useWatch("repeat", form);
  const targetId = Form.useWatch("target_id", form);
  const [showAllDrives, setShowAllDrives] = useState(false);
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
      setShowAllDrives(false);
    }
  }, [job, form]);

  if (!job) return null;

  // Mặc định chỉ hiện ổ đã đánh dấu; bật "tất cả ổ đĩa" thì thêm cả ổ cứng trong máy
  const usbOptions = props.drives
    .filter((d) => d.marker || showAllDrives)
    .map((d) =>
      d.marker
        ? { value: d.marker.id, label: `${d.marker.label} — ${d.mount_point}` }
        : {
            value: UNMARKED + d.mount_point,
            label: `${d.name || (d.removable ? "Ổ rời" : "Ổ đĩa")} — ${d.mount_point} · trống ${bytes(d.available_space)}`,
          },
    );
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
      if (v.kind === "usb" && v.target_id?.startsWith(UNMARKED)) {
        // Ổ chưa đánh dấu: ghi .backup-id trước để có UUID nhận diện
        const mount = v.target_id.slice(UNMARKED.length);
        const drive = props.drives.find((d) => d.mount_point === mount);
        const marker = await api.markTarget(mount, drive?.name || mount);
        v = { ...v, target_id: marker.id };
        form.setFieldValue("target_id", marker.id);
        props.onDrivesChanged();
      }
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
    // FTP không có sự kiện "cắm ổ" -> gợi ý chạy mỗi giờ
    if (k === "ftp" && form.getFieldValue("repeat") === "none") {
      form.setFieldsValue({ repeat: "interval", interval_minutes: 60 });
    }
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
              { value: "usb", label: "Ổ đĩa / USB", icon: <UsbOutlined /> },
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
              <Space orientation="vertical" size={2}>
                <Checkbox
                  checked={showAllDrives}
                  onChange={(e) => {
                    setShowAllDrives(e.target.checked);
                    if (!e.target.checked && targetId?.startsWith(UNMARKED)) {
                      form.setFieldValue("target_id", undefined);
                    }
                  }}
                >
                  Hiện tất cả ổ đĩa (cả ổ cứng trong máy, ổ chưa đánh dấu)
                </Checkbox>
                {targetId?.startsWith(UNMARKED) ? (
                  <span>
                    Ổ này chưa đánh dấu. Khi lưu, app ghi file <code>.backup-id</code> vào gốc ổ để nhận diện.
                  </span>
                ) : usbOptions.length === 0 ? (
                  <span>Chưa có ổ đích. Cắm ổ USB, hoặc tick ô trên để chọn ổ bất kỳ.</span>
                ) : null}
              </Space>
            }
          >
            <Select options={usbOptions} placeholder="Chọn ổ đích" />
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
          name="repeat"
          label="Chạy định kỳ"
          extra={
            repeat === "none"
              ? kind === "usb"
                ? undefined
                : "Chỉ chạy thủ công."
              : kind === "usb"
                ? "Chỉ chạy khi ổ đang cắm."
                : undefined
          }
        >
          <Segmented<Repeat>
            options={[
              { value: "none", label: "Tắt" },
              { value: "interval", label: "Mỗi N phút" },
              { value: "daily", label: "Hằng ngày" },
              { value: "weekly", label: "Hằng tuần" },
              { value: "monthly", label: "Hằng tháng" },
            ]}
          />
        </Form.Item>
        {repeat === "interval" && (
          <Form.Item name="interval_minutes" label="Mỗi" rules={[{ required: true, message: "Nhập số phút" }]}>
            <InputNumber min={1} max={10080} suffix="phút" style={{ width: 160 }} />
          </Form.Item>
        )}
        {repeat === "weekly" && (
          <Form.Item
            name="weekdays"
            label="Vào các thứ"
            rules={[{ required: true, type: "array", min: 1, message: "Chọn ít nhất một thứ" }]}
          >
            <Checkbox.Group options={WEEKDAYS.map((label, i) => ({ label, value: i + 1 }))} />
          </Form.Item>
        )}
        {repeat === "monthly" && (
          <Form.Item
            name="month_day"
            label="Vào ngày"
            rules={[{ required: true, message: "Nhập ngày trong tháng" }]}
            extra="Tháng không có ngày này thì chạy vào ngày cuối tháng."
          >
            <InputNumber min={1} max={31} style={{ width: 160 }} />
          </Form.Item>
        )}
        {(repeat === "daily" || repeat === "weekly" || repeat === "monthly") && (
          <Form.Item
            name="at"
            label="Lúc"
            rules={[{ required: true, message: "Chọn giờ chạy" }]}
            getValueProps={(v?: string) => ({ value: toTime(v) })}
            normalize={(v: Dayjs | null) => (v ? v.format("HH:mm") : undefined)}
            extra="Theo giờ của máy. Lỡ giờ (máy tắt, app chưa mở, ổ chưa cắm) thì chạy bù ngay khi có thể."
          >
            <TimePicker format="HH:mm" allowClear={false} needConfirm={false} style={{ width: 160 }} />
          </Form.Item>
        )}

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
