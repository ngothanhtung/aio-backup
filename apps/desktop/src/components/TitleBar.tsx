import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";

/** macOS giữ nút đèn giao thông của hệ thống, không vẽ nút riêng. */
export const isMac = navigator.userAgent.includes("Mac");

const icon = (d: string) => (
  <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1">
    <path d={d} />
  </svg>
);

/** Thanh tiêu đề tự vẽ: vùng kéo cửa sổ + nút thu nhỏ / phóng to / đóng. */
export default function TitleBar() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    const sync = () => {
      win.isMaximized().then(setMaximized).catch(() => {});
    };
    sync();
    const unlisten = win.onResized(sync);
    return () => {
      unlisten.then((f) => f());
    };
  }, []);

  const win = getCurrentWindow();
  return (
    <header className="titlebar" data-tauri-drag-region>
      {!isMac && (
        <div className="win-controls">
          <button title="Thu nhỏ" onClick={() => win.minimize()}>
            {icon("M0 5.5h10")}
          </button>
          <button title={maximized ? "Khôi phục" : "Phóng to"} onClick={() => win.toggleMaximize()}>
            {icon(maximized ? "M2.5 2.5v-2h7v7h-2M0.5 2.5h7v7h-7z" : "M0.5 0.5h9v9h-9z")}
          </button>
          <button className="close" title="Đóng (ẩn xuống khay hệ thống)" onClick={() => win.close()}>
            {icon("M0 0l10 10M10 0L0 10")}
          </button>
        </div>
      )}
    </header>
  );
}
