import { useEffect, useMemo, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Checkbox,
  Col,
  Empty,
  Input,
  Progress,
  Result,
  Row,
  Select,
  Space,
  Spin,
  Tree,
  Typography,
} from "antd";
import type { TreeDataNode } from "antd";
import { FileOutlined, FolderOpenOutlined, FolderOutlined, SearchOutlined } from "@ant-design/icons";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import dayjs from "dayjs";
import {
  api,
  bytes,
  errText,
  type RestoreEntry,
  type RestoreProgress,
  type RestoreReport,
  type SnapshotInfo,
} from "../api";
import type { PageProps } from "../App";

const CURRENT = "__current__";

/** Dựng cây thư mục từ danh sách đường dẫn `a/b/c.txt`. */
function buildTree(entries: RestoreEntry[]): TreeDataNode[] {
  const root: TreeDataNode[] = [];
  const dirs = new Map<string, TreeDataNode>();
  const childrenOf = (dir: string): TreeDataNode[] => {
    if (!dir) return root;
    let node = dirs.get(dir);
    if (!node) {
      const idx = dir.lastIndexOf("/");
      const name = dir.slice(idx + 1);
      node = { key: `d:${dir}`, title: name, icon: <FolderOutlined />, children: [] };
      dirs.set(dir, node);
      childrenOf(idx < 0 ? "" : dir.slice(0, idx)).push(node);
    }
    return node.children!;
  };
  for (const e of entries) {
    const idx = e.path.lastIndexOf("/");
    const dir = idx < 0 ? "" : e.path.slice(0, idx);
    childrenOf(dir).push({
      key: `f:${e.path}`,
      isLeaf: true,
      icon: <FileOutlined />,
      title: (
        <span>
          {e.path.slice(idx + 1)}{" "}
          <Typography.Text type="secondary" style={{ fontSize: 12 }}>
            {bytes(e.size)}
            {e.modified ? ` · ${dayjs(e.modified).format("DD/MM/YYYY HH:mm")}` : ""}
          </Typography.Text>
        </span>
      ),
    });
  }
  // Thư mục trước, file sau
  const sort = (nodes: TreeDataNode[]) => {
    nodes.sort((a, b) => Number(!!a.isLeaf) - Number(!!b.isLeaf) || String(a.key).localeCompare(String(b.key)));
    nodes.forEach((n) => n.children && sort(n.children));
  };
  sort(root);
  return root;
}

export default function Restore({ data, initialJobId }: PageProps & { initialJobId?: string }) {
  const { config } = data;
  const [jobId, setJobId] = useState<string | undefined>(initialJobId ?? config.jobs[0]?.id);
  const job = config.jobs.find((j) => j.id === jobId);

  const [snapshots, setSnapshots] = useState<SnapshotInfo[]>([]);
  const [version, setVersion] = useState<string>(CURRENT);
  const [entries, setEntries] = useState<RestoreEntry[]>([]);
  const [loading, setLoading] = useState(false);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [checked, setChecked] = useState<string[]>([]);
  const [target, setTarget] = useState("");
  const [overwrite, setOverwrite] = useState(false);
  const [progress, setProgress] = useState<RestoreProgress | null>(null);
  const [report, setReport] = useState<RestoreReport | null>(null);
  const [restoring, setRestoring] = useState(false);

  useEffect(() => {
    if (initialJobId) setJobId(initialJobId);
  }, [initialJobId]);

  // Đổi job -> tải danh sách snapshot
  useEffect(() => {
    setSnapshots([]);
    setVersion(CURRENT);
    setReport(null);
    if (!jobId) return;
    api
      .listSnapshots(jobId)
      .then(setSnapshots)
      .catch(() => setSnapshots([]));
  }, [jobId]);

  // Đổi job/phiên bản -> tải danh sách file
  useEffect(() => {
    setEntries([]);
    setChecked([]);
    setLoadError(null);
    setReport(null);
    if (!jobId) return;
    setLoading(true);
    api
      .listRestoreEntries(jobId, version === CURRENT ? null : version)
      .then(setEntries)
      .catch((e) => setLoadError(errText(e)))
      .finally(() => setLoading(false));
  }, [jobId, version]);

  useEffect(() => {
    const un = listen<RestoreProgress>("restore-progress", (e) => setProgress(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, []);

  const filtered = useMemo(() => {
    const q = search.trim().toLowerCase();
    return q ? entries.filter((e) => e.path.toLowerCase().includes(q)) : entries;
  }, [entries, search]);
  const tree = useMemo(() => buildTree(filtered), [filtered]);
  const sizeOf = useMemo(() => new Map(entries.map((e) => [e.path, e.size])), [entries]);

  const selectedPaths = checked.filter((k) => k.startsWith("f:")).map((k) => k.slice(2));
  const selectedBytes = selectedPaths.reduce((s, p) => s + (sizeOf.get(p) ?? 0), 0);
  const toOriginal = !!job && target.trim() === job.source;

  const pickTarget = async () => {
    const dir = await open({ directory: true, multiple: false, title: "Chọn thư mục nhận file khôi phục" });
    if (typeof dir === "string") setTarget(dir);
  };

  const doRestore = async () => {
    if (!jobId) return;
    setRestoring(true);
    setReport(null);
    setProgress({ done: 0, total: selectedPaths.length, current: "" });
    try {
      const r = await api.restoreFiles({
        jobId,
        snapshot: version === CURRENT ? null : version,
        paths: selectedPaths,
        target: target.trim(),
        overwrite,
      });
      setReport(r);
    } catch (e) {
      setReport({ restored: 0, bytes: 0, skipped: [], failed: [{ rel: "", error: errText(e) }], cancelled: false });
    } finally {
      setRestoring(false);
      setProgress(null);
    }
  };

  if (config.jobs.length === 0) {
    return (
      <div className="page">
        <div className="page-head">
          <h1>Khôi phục</h1>
        </div>
        <Empty description="Chưa có job nào để khôi phục" />
      </div>
    );
  }

  const versionOptions = [
    { value: CURRENT, label: "Bản backup hiện tại" },
    ...snapshots.map((s) => ({
      value: s.name,
      label: `Bản cũ lúc ${s.takenAt ? dayjs(s.takenAt).format("HH:mm:ss DD/MM/YYYY") : s.name}`,
    })),
  ];

  return (
    <div className="page">
      <div className="page-head">
        <h1>Khôi phục</h1>
      </div>

      <Row gutter={[16, 16]}>
        <Col xs={24} lg={15}>
          <Card
            title="Chọn file"
            extra={
              <Typography.Text type="secondary">
                {entries.length} file{selectedPaths.length ? ` · đã chọn ${selectedPaths.length}` : ""}
              </Typography.Text>
            }
          >
            <Space orientation="vertical" style={{ width: "100%" }} size={12}>
              <Space wrap>
                <Select
                  style={{ width: 220 }}
                  value={jobId}
                  onChange={setJobId}
                  options={config.jobs.map((j) => ({ value: j.id, label: j.name }))}
                />
                <Select style={{ width: 280 }} value={version} onChange={setVersion} options={versionOptions} />
              </Space>
              {version !== CURRENT && (
                <Typography.Text type="secondary">
                  Bản cũ chỉ chứa những file đã bị ghi đè hoặc xoá ở lần backup đó.
                </Typography.Text>
              )}
              <Input
                allowClear
                prefix={<SearchOutlined />}
                placeholder="Tìm theo tên hoặc đường dẫn"
                value={search}
                onChange={(e) => setSearch(e.target.value)}
              />
              <Space>
                <Button
                  size="small"
                  onClick={() => setChecked(filtered.map((e) => `f:${e.path}`))}
                  disabled={!filtered.length}
                >
                  Chọn tất cả{search ? " (kết quả tìm)" : ""}
                </Button>
                <Button size="small" onClick={() => setChecked([])} disabled={!checked.length}>
                  Bỏ chọn
                </Button>
              </Space>
              <div className="tree-wrap">
                {loading ? (
                  <div style={{ textAlign: "center", padding: 40 }}>
                    <Spin description="Đang đọc danh sách file…" />
                  </div>
                ) : loadError ? (
                  <Alert type="error" showIcon title="Không đọc được bản backup" description={loadError} />
                ) : tree.length === 0 ? (
                  <Empty description={search ? "Không có file khớp" : "Chưa có file nào trong bản backup này"} />
                ) : (
                  <Tree
                    checkable
                    showIcon
                    selectable={false}
                    height={420}
                    treeData={tree}
                    defaultExpandAll={filtered.length < 200}
                    checkedKeys={checked}
                    onCheck={(keys) => setChecked((Array.isArray(keys) ? keys : keys.checked).map(String))}
                  />
                )}
              </div>
            </Space>
          </Card>
        </Col>

        <Col xs={24} lg={9}>
          <Card title="Khôi phục về">
            <Space orientation="vertical" style={{ width: "100%" }} size={12}>
              <Space.Compact style={{ width: "100%" }}>
                <Input placeholder="Thư mục nhận file" value={target} onChange={(e) => setTarget(e.target.value)} />
                <Button icon={<FolderOpenOutlined />} onClick={pickTarget}>
                  Chọn…
                </Button>
              </Space.Compact>
              {job && (
                <Button type="link" size="small" style={{ padding: 0 }} onClick={() => setTarget(job.source)}>
                  Dùng thư mục gốc của job
                </Button>
              )}
              <Checkbox checked={overwrite} onChange={(e) => setOverwrite(e.target.checked)}>
                Ghi đè file đã có
              </Checkbox>
              {toOriginal && overwrite && (
                <Alert
                  type="warning"
                  showIcon
                  title="File hiện tại trong thư mục gốc sẽ bị thay bằng bản khôi phục."
                />
              )}
              <Typography.Text type="secondary">
                Giữ nguyên cấu trúc thư mục con. {selectedPaths.length} file · {bytes(selectedBytes)}
              </Typography.Text>
              {restoring ? (
                <>
                  <Progress
                    percent={progress && progress.total ? Math.floor((progress.done / progress.total) * 100) : 0}
                    status="active"
                  />
                  <Typography.Text type="secondary" ellipsis style={{ fontSize: 12 }}>
                    {progress?.current}
                  </Typography.Text>
                  <Button block onClick={() => api.cancelRestore()}>
                    Huỷ
                  </Button>
                </>
              ) : (
                <Button
                  type="primary"
                  block
                  disabled={!selectedPaths.length || !target.trim()}
                  onClick={doRestore}
                >
                  Khôi phục {selectedPaths.length || ""} file
                </Button>
              )}
            </Space>
          </Card>

          {report && (
            <Card style={{ marginTop: 16 }}>
              <Result
                style={{ padding: 0 }}
                status={report.failed.length ? (report.restored ? "warning" : "error") : "success"}
                title={
                  report.failed.length && !report.restored
                    ? "Khôi phục thất bại"
                    : `Đã khôi phục ${report.restored} file (${bytes(report.bytes)})`
                }
                subTitle={[
                  report.skipped.length ? `${report.skipped.length} file đã có nên bỏ qua` : "",
                  report.cancelled ? "Đã huỷ giữa chừng" : "",
                ]
                  .filter(Boolean)
                  .join(" · ")}
              />
              {report.failed.length > 0 && (
                <ul style={{ paddingLeft: 18, marginBottom: 0 }}>
                  {report.failed.slice(0, 50).map((f, i) => (
                    <li key={i}>
                      <Typography.Text type="danger">
                        {f.rel ? <span className="mono">{f.rel}</span> : null} {f.error}
                      </Typography.Text>
                    </li>
                  ))}
                </ul>
              )}
            </Card>
          )}
        </Col>
      </Row>
    </div>
  );
}
