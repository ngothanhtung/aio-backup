// Trạng thái dùng chung của app + lắng nghe event từ backend.
import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import {
  api,
  type Config,
  type DriveInfo,
  type FinishedEvent,
  type HistoryEntry,
  type ProgressEvent,
  type Report,
  type ScheduleInfo,
  type Uuid,
} from "./api";

export interface RunState {
  dryRun: boolean;
  totalFiles: number;
  totalBytes: number;
  doneFiles: number;
  doneBytes: number;
  current: string;
}

export interface LastResult {
  report: Report | null;
  error: string | null;
}

const emptyRun = (dryRun = false): RunState => ({
  dryRun,
  totalFiles: 0,
  totalBytes: 0,
  doneFiles: 0,
  doneBytes: 0,
  current: "",
});

export function useAppData(onFinished?: (e: FinishedEvent) => void) {
  const [config, setConfig] = useState<Config>({
    poll_interval_secs: 5,
    default_excludes: [],
    default_keep_days: 30,
    jobs: [],
  });
  const [drives, setDrives] = useState<DriveInfo[]>([]);
  const [running, setRunning] = useState<Record<Uuid, RunState>>({});
  const [results, setResults] = useState<Record<Uuid, LastResult>>({});
  const [history, setHistory] = useState<HistoryEntry[]>([]);
  const [schedule, setSchedule] = useState<ScheduleInfo[]>([]);
  const [loaded, setLoaded] = useState(false);

  const refreshHistory = useCallback(() => api.getHistory(1000).then(setHistory), []);
  const refreshSchedule = useCallback(() => api.getSchedule().then(setSchedule), []);
  const refreshDrives = useCallback(() => api.getDrives().then(setDrives), []);

  useEffect(() => {
    Promise.all([
      api.getConfig().then(setConfig),
      refreshDrives(),
      refreshHistory(),
      refreshSchedule(),
      api.runningJobs().then((ids) => setRunning(Object.fromEntries(ids.map((id) => [id, emptyRun()])))),
    ]).finally(() => setLoaded(true));

    const unsubs = [
      listen<DriveInfo[]>("drives-changed", (e) => {
        setDrives(e.payload);
        refreshSchedule();
      }),
      listen<Uuid>("job-started", (e) =>
        setRunning((r) => ({ ...r, [e.payload]: r[e.payload] ?? emptyRun() })),
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
          const { [payload.jobId]: _done, ...rest } = r;
          return rest;
        });
        setResults((x) => ({ ...x, [payload.jobId]: { report: payload.report, error: payload.error } }));
        refreshHistory();
        refreshSchedule();
        onFinished?.(payload);
      }),
    ];
    // Cập nhật "lần kế tiếp" định kỳ
    const timer = window.setInterval(refreshSchedule, 30_000);
    return () => {
      unsubs.forEach((p) => p.then((f) => f()));
      window.clearInterval(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const markRunning = useCallback(
    (id: Uuid, dryRun: boolean) => setRunning((r) => ({ ...r, [id]: emptyRun(dryRun) })),
    [],
  );
  const clearRunning = useCallback(
    (id: Uuid) =>
      setRunning((r) => {
        const { [id]: _gone, ...rest } = r;
        return rest;
      }),
    [],
  );

  /** Lượt chạy thật gần nhất của từng job (lấy từ lịch sử). */
  const lastRunByJob = useMemo(() => {
    const m = new Map<Uuid, HistoryEntry>();
    for (const e of history) {
      if (e.report.dryRun) continue;
      if (!m.has(e.jobId)) m.set(e.jobId, e);
    }
    return m;
  }, [history]);

  const targets = useMemo(() => drives.filter((d) => d.marker), [drives]);
  const presentTargets = useMemo(
    () => new Map(targets.map((d) => [d.marker!.id, d] as const)),
    [targets],
  );

  return {
    loaded,
    config,
    setConfig,
    drives,
    refreshDrives,
    targets,
    presentTargets,
    running,
    markRunning,
    clearRunning,
    results,
    history,
    refreshHistory,
    schedule,
    refreshSchedule,
    lastRunByJob,
  };
}

export type AppData = ReturnType<typeof useAppData>;
