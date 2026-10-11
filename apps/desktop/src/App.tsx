import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { confirm, open } from "@tauri-apps/plugin-dialog";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import {
  api,
  bytes,
  errText,
  newJob,
  type Config,
  type DriveInfo,
  type FinishedEvent,
  type HistoryEntry,
  type Job,
  type ProgressEvent,
  type Report,
  type Uuid,
} from "./api";

interface RunState {
  totalFiles: number;
  totalBytes: number;
  doneFiles: number;
  doneBytes: number;
  current: string;
}

interface LastResult {
  report: Report | null;
  error: string | null;
}

export default function App() {
  const [config, setConfig] = useState<Config>({ poll_interval_secs: 5, jobs: [] });
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  const [running, setRunning] = useState<Record<Uuid, RunState>>({});
  const [results, setResults] = useState<Record<Uuid, LastResult>>({});
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [editing, setEditing] = useState<Job | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [autostart, setAutostart] = useState(false);

  const flash = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast(null), 4000);
  }, []);

  const refreshHistory = useCallback(() => api.getHistory(30).then(setHistory), []);

  useEffect(() => {
    api.getConfig().then(setConfig);
    api.getDrives().then(setDrives);
    refreshHistory();
    api.runningJobs().then((ids) =>
      setRunning(Object.fromEntries(ids.map((id) => [id, emptyRun()]))),
    );
    isEnabled().then(setAutostart).catch(() => {});

    const unsubs = [
      listen<DriveInfo[]>("drives-changed", (e) => setDrives(e.payload)),
      listen<Uuid>("job-started", (e) =>
        setRunning((r) => ({ ...r, [e.payload]: emptyRun() })),
      ),
      listen<ProgressEvent>("job-progress", ({ payload: { jobId, progress: p } }) =>
        setRunning((r) => {
          const cur = r[jobId] ?? emptyRun();
          if (p.type === "started")
            return { ...r, [jobId]: { ...cur, totalFiles: p.totalFiles, totalBytes: p.totalBytes } };
          if (p.type === "file")
            return {
              ...r,
              [jobId]: { ...cur, doneFiles: p.doneFiles, doneBytes: p.doneBytes, current: p.rel },
            };
          return r;
        }),
      ),
      listen<FinishedEvent>("job-finished", ({ payload }) => {
        setRunning((r) => {
          const { [payload.jobId]: _, ...rest } = r;
          return rest;
        });
        setResults((x) => ({
          ...x,
          [payload.jobId]: { report: payload.report, error: payload.error },
        }));
        refreshHistory();
      }),
    ];
    return () => unsubs.forEach((p) => p.then((f) => f()));
  }, [refreshHistory]);

  const presentTargets = useMemo(
    () => new Map(drives.filter((d) => d.marker).map((d) => [d.marker!.id, d])),
    [drives],
  );
  const targets = useMemo(() => drives.filter((d) => d.marker), [drives]);

  const run = async (job: Job, dryRun: boolean) => {
    try {
      await api.startJob(job.id, dryRun);
    } catch (e) {
      flash(errText(e));
    }
  };

  const remove = async (job: Job) => {
    if (!(await confirm(`Xoá job "${job.name}"? Dữ liệu trên USB vẫn giữ nguyên.`))) return;
    setConfig(await api.deleteJob(job.id));
  };

  const toggleAutostart = async () => {
    try {
      autostart ? await disable() : await enable();
      setAutostart(await isEnabled());
    } catch (e) {
      flash(errText(e));
    }
  };

  return (
    <div className="app">
      <header>
        <div>
          <h1>USB Backup</h1>
          <p className="muted">Cắm ổ đích là tự backup · đóng cửa sổ app vẫn chạy dưới khay hệ thống</p>
        </div>
        <label className="switch">
          <input type="checkbox" checked={autostart} onChange={toggleAutostart} />
          Khởi động cùng máy
        </label>
      </header>

      <main>
        <section>
          <div className="section-head">
            <h2>Job backup</h2>
            <button
              className="primary"
              disabled={targets.length === 0}
              title={targets.length === 0 ? "Cần đánh dấu một ổ đích trước" : ""}
              onClick={() => setEditing(newJob(targets[0]?.marker!.id ?? ""))}
            >
              + Thêm job
            </button>
          </div>

          {config.jobs.length === 0 && (
            <div className="empty">
              {targets.length === 0
                ? "Bước 1: cắm ổ USB và bấm “Đánh dấu làm ổ đích” ở cột bên phải."
                : "Bước 2: thêm job chọn thư mục cần backup."}
            </div>
          )}

          {config.jobs.map((job) => (
            <JobCard
              key={job.id}
              job={job}
              drive={presentTargets.get(job.target_id)}
              run={running[job.id]}
              result={results[job.id]}
              onRun={(dry) => run(job, dry)}
              onCancel={() => api.cancelJob(job.id)}
              onEdit={() => setEditing(structuredClone(job))}
              onDelete={() => remove(job)}
            />
          ))}

          <h2 className="spaced">Lịch sử</h2>
          <HistoryList entries={history} />
        </section>

        <aside>
          <h2>Ổ đĩa</h2>
          <DriveList
            drives={drives}
            onMarked={async () => setDrives(await api.getDrives())}
            onError={flash}
          />
        </aside>
      </main>

      {editing && (
        <JobForm
          job={editing}
          targets={targets}
          onCancel={() => setEditing(null)}
          onSave={async (job) => {
            try {
              setConfig(await api.saveJob(job));
              setEditing(null);
            } catch (e) {
              flash(errText(e));
            }
          }}
        />
      )}

      {toast && <div className="toast">{toast}</div>}
    </div>
  );
}

const emptyRun = (): RunState => ({
  totalFiles: 0,
  totalBytes: 0,
  doneFiles: 0,
  doneBytes: 0,
  current: "",
});

function JobCard(props: {
  job: Job;
  drive?: DriveInfo;
  run?: RunState;
  result?: LastResult;
  onRun: (dryRun: boolean) => void;
  onCancel: () => void;
  onEdit: () => void;
  onDelete: () => void;
}) {
  const { job, drive, run, result } = props;
  const pct = run && run.totalBytes > 0 ? Math.round((run.doneBytes / run.totalBytes) * 100) : 0;

  return (
    <article className="card">
      <div className="card-head">
        <div>
          <h3>
            <span className={`dot ${drive ? "on" : ""}`} />
            {job.name}
          </h3>
          <p className="path">{job.source}</p>
          <p className="muted small">
            → {drive ? `${drive.marker!.label} (${drive.mount_point})` : "Ổ đích chưa cắm"}
            {" · "}
            {job.mirror_deletes ? "mirror" : "chỉ thêm/cập nhật"}
            {job.versioning ? " · có versioning" : ""}
            {job.auto_run ? " · tự chạy" : ""}
          </p>
        </div>
        <div className="actions">
          {run ? (
            <button onClick={props.onCancel}>Huỷ</button>
          ) : (
            <>
              <button disabled={!drive} onClick={() => props.onRun(true)}>
                Xem trước
              </button>
              <button className="primary" disabled={!drive} onClick={() => props.onRun(false)}>
                Chạy
              </button>
              <button className="ghost" onClick={props.onEdit}>
                Sửa
              </button>
              <button className="ghost danger" onClick={props.onDelete}>
                Xoá
              </button>
            </>
          )}
        </div>
      </div>

      {run && (
        <div className="progress">
          <div className="bar">
            <div style={{ width: `${pct}%` }} />
          </div>
          <p className="muted small">
            {run.doneFiles}/{run.totalFiles} file · {bytes(run.doneBytes)}/{bytes(run.totalBytes)}
            {run.current && ` · ${run.current}`}
          </p>
        </div>
      )}

      {!run && result && <ResultLine {...result} />}
    </article>
  );
}

function ResultLine({ report, error }: LastResult) {
  if (error) return <p className="result bad">Lỗi: {error}</p>;
  if (!report) return null;
  const prefix = report.dryRun ? "Xem trước: sẽ " : "Xong: ";
  const ok = report.failed.length === 0 && !report.cancelled && !report.targetLost;
  return (
    <div className={`result ${ok ? "good" : "bad"}`}>
      {prefix}+{report.created} mới, ~{report.updated} cập nhật, -{report.deleted} xoá ·{" "}
      {bytes(report.bytesCopied)}
      {report.cancelled && " · đã huỷ"}
      {report.targetLost && " · ổ đích bị rút giữa chừng"}
      {report.failed.length > 0 && ` · ${report.failed.length} lỗi`}
      {(report.warnings.length > 0 || report.failed.length > 0) && (
        <details>
          <summary>Chi tiết</summary>
          <ul>
            {report.failed.map((f) => (
              <li key={f.rel}>
                ✗ {f.rel}: {f.error}
              </li>
            ))}
            {report.warnings.map((w, i) => (
              <li key={i}>! {w}</li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}

function DriveList(props: {
  drives: DriveInfo[];
  onMarked: () => void;
  onError: (m: string) => void;
}) {
  const [labeling, setLabeling] = useState<string | null>(null);
  const [label, setLabel] = useState("");
  const [showAll, setShowAll] = useState(false);

  // Mặc định chỉ hiện ổ rời và ổ đã đánh dấu, tránh rối với phân vùng hệ thống
  const visible = showAll ? props.drives : props.drives.filter((d) => d.removable || d.marker);

  const mark = async (d: DriveInfo) => {
    try {
      await api.markTarget(d.mount_point, label.trim() || d.name || "USB Backup");
      setLabeling(null);
      props.onMarked();
    } catch (e) {
      props.onError(errText(e));
    }
  };

  return (
    <div className="drives">
      {visible.length === 0 && <p className="muted small">Chưa thấy ổ USB nào.</p>}
      {visible.map((d) => {
        const used = d.total_space > 0 ? 1 - d.available_space / d.total_space : 0;
        return (
          <div key={d.mount_point} className={`drive ${d.marker ? "target" : ""}`}>
            <div className="drive-name">
              {d.marker ? d.marker.label : d.name || d.mount_point}
              {d.marker && <span className="badge">ĐÍCH</span>}
            </div>
            <div className="muted small">
              {d.mount_point} · {d.file_system}
            </div>
            <div className="bar thin">
              <div style={{ width: `${Math.round(used * 100)}%` }} />
            </div>
            <div className="muted small">
              Trống {bytes(d.available_space)} / {bytes(d.total_space)}
            </div>
            {!d.marker &&
              (labeling === d.mount_point ? (
                <div className="inline-form">
                  <input
                    autoFocus
                    placeholder="Tên ổ, vd: Samsung T7"
                    value={label}
                    onChange={(e) => setLabel(e.target.value)}
                    onKeyDown={(e) => e.key === "Enter" && mark(d)}
                  />
                  <button className="primary" onClick={() => mark(d)}>
                    OK
                  </button>
                </div>
              ) : (
                <button
                  className="small-btn"
                  onClick={() => {
                    setLabel(d.name);
                    setLabeling(d.mount_point);
                  }}
                >
                  Đánh dấu làm ổ đích
                </button>
              ))}
          </div>
        );
      })}
      <label className="muted small check">
        <input type="checkbox" checked={showAll} onChange={(e) => setShowAll(e.target.checked)} />
        Hiện cả ổ hệ thống
      </label>
    </div>
  );
}

function JobForm(props: {
  job: Job;
  targets: DriveInfo[];
  onCancel: () => void;
  onSave: (job: Job) => void;
}) {
  const [job, setJob] = useState<Job>(props.job);
  const [excludes, setExcludes] = useState(props.job.excludes.join("\n"));
  const set = <K extends keyof Job>(k: K, v: Job[K]) => setJob((j) => ({ ...j, [k]: v }));

  const pickSource = async () => {
    const dir = await open({ directory: true, multiple: false, title: "Chọn thư mục cần backup" });
    if (typeof dir === "string") {
      setJob((j) => ({
        ...j,
        source: dir,
        name: j.name || dir.split(/[\\/]/).filter(Boolean).pop() || "",
      }));
    }
  };

  const submit = () =>
    props.onSave({
      ...job,
      excludes: excludes
        .split("\n")
        .map((s) => s.trim())
        .filter(Boolean),
    });

  return (
    <div className="modal-backdrop" onClick={props.onCancel}>
      <div className="modal" onClick={(e) => e.stopPropagation()}>
        <h2>{props.job.name ? "Sửa job" : "Job mới"}</h2>

        <label>
          Thư mục nguồn
          <div className="row">
            <input value={job.source} onChange={(e) => set("source", e.target.value)} />
            <button onClick={pickSource}>Chọn…</button>
          </div>
        </label>

        <label>
          Tên job
          <input value={job.name} onChange={(e) => set("name", e.target.value)} />
        </label>

        <label>
          Ổ đích
          <select value={job.target_id} onChange={(e) => set("target_id", e.target.value)}>
            {!props.targets.some((t) => t.marker!.id === job.target_id) && (
              <option value={job.target_id}>(ổ hiện chưa cắm)</option>
            )}
            {props.targets.map((t) => (
              <option key={t.marker!.id} value={t.marker!.id}>
                {t.marker!.label} — {t.mount_point}
              </option>
            ))}
          </select>
        </label>

        <label>
          Thư mục con trên ổ đích <span className="muted">(để trống = dùng tên job)</span>
          <input value={job.dest_subdir} onChange={(e) => set("dest_subdir", e.target.value)} />
        </label>

        <label>
          Loại trừ <span className="muted">(mỗi dòng một pattern glob)</span>
          <textarea rows={4} value={excludes} onChange={(e) => setExcludes(e.target.value)} />
        </label>

        <div className="checks">
          <label className="check">
            <input
              type="checkbox"
              checked={job.auto_run}
              onChange={(e) => set("auto_run", e.target.checked)}
            />
            Tự chạy khi cắm ổ đích
          </label>
          <label className="check">
            <input
              type="checkbox"
              checked={job.versioning}
              onChange={(e) => set("versioning", e.target.checked)}
            />
            Giữ bản cũ trong .versions
          </label>
          {job.versioning && (
            <label className="check">
              Xoá bản cũ sau
              <input
                type="number"
                min={0}
                className="num"
                value={job.keep_versions_days ?? 0}
                onChange={(e) => {
                  const n = Number(e.target.value);
                  set("keep_versions_days", n > 0 ? n : null);
                }}
              />
              ngày (0 = giữ mãi)
            </label>
          )}
          <label className="check">
            <input
              type="checkbox"
              checked={job.mirror_deletes}
              onChange={(e) => set("mirror_deletes", e.target.checked)}
            />
            Mirror: xoá ở USB file đã xoá ở nguồn
          </label>
          <label className="check">
            <input
              type="checkbox"
              checked={job.verify_hash}
              onChange={(e) => set("verify_hash", e.target.checked)}
            />
            So nội dung (blake3) khi chỉ khác thời gian sửa
          </label>
        </div>

        <div className="modal-actions">
          <button className="ghost" onClick={props.onCancel}>
            Huỷ
          </button>
          <button className="primary" disabled={!job.source || !job.name} onClick={submit}>
            Lưu
          </button>
        </div>
      </div>
    </div>
  );
}

function HistoryList({ entries }: { entries: HistoryEntry[] }) {
  if (entries.length === 0) return <p className="muted small">Chưa có lượt chạy nào.</p>;
  return (
    <table className="history">
      <thead>
        <tr>
          <th>Thời điểm</th>
          <th>Job</th>
          <th>Kết quả</th>
          <th>Dung lượng</th>
        </tr>
      </thead>
      <tbody>
        {entries.map((e, i) => {
          const r = e.report;
          return (
            <tr key={i}>
              <td>{new Date(r.startedAt).toLocaleString("vi-VN")}</td>
              <td>
                {e.jobName}
                {r.dryRun && <span className="muted"> (xem trước)</span>}
              </td>
              <td className={r.failed.length || r.cancelled || r.targetLost ? "bad" : ""}>
                +{r.created} ~{r.updated} -{r.deleted}
                {r.failed.length > 0 && ` · ${r.failed.length} lỗi`}
                {r.cancelled && " · huỷ"}
              </td>
              <td>{bytes(r.bytesCopied)}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
