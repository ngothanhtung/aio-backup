import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { App as AntApp, ConfigProvider, theme } from "antd";
import viVN from "antd/locale/vi_VN";
import dayjs from "dayjs";
import "dayjs/locale/vi";
import relativeTime from "dayjs/plugin/relativeTime";
import App from "./App";
import { isMac } from "./components/TitleBar";
import "./styles.css";

dayjs.extend(relativeTime);
dayjs.locale("vi");

const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");
const applyTheme = (dark: boolean) => {
  document.documentElement.dataset.theme = dark ? "dark" : "light";
};
// Đặt trước lần vẽ đầu để khung app không nháy sai màu
applyTheme(darkQuery.matches);
document.documentElement.classList.toggle("is-mac", isMac);

// Bản phát hành: bỏ menu chuột phải của webview (Reload, Inspect...), trừ trong ô nhập liệu
if (import.meta.env.PROD) {
  window.addEventListener("contextmenu", (e) => {
    if (!(e.target as HTMLElement).closest("input, textarea")) e.preventDefault();
  });
}

/** Theo chế độ sáng/tối của hệ điều hành. */
function useSystemDark() {
  const [dark, setDark] = useState(darkQuery.matches);
  useEffect(() => {
    const on = (e: MediaQueryListEvent) => setDark(e.matches);
    darkQuery.addEventListener("change", on);
    return () => darkQuery.removeEventListener("change", on);
  }, []);
  return dark;
}

function Root() {
  const dark = useSystemDark();
  useEffect(() => applyTheme(dark), [dark]);
  return (
    <ConfigProvider
      locale={viVN}
      // Hiệu ứng gợn sóng khi bấm nút là nét của web, app desktop không có
      wave={{ disabled: true }}
      theme={{
        algorithm: dark ? theme.darkAlgorithm : theme.defaultAlgorithm,
        token: {
          colorPrimary: dark ? "#5b8cff" : "#2563eb",
          colorSuccess: dark ? "#3fb950" : "#1a7f37",
          colorWarning: dark ? "#d29922" : "#bf8700",
          colorError: dark ? "#f85149" : "#cf222e",
          borderRadius: 8,
          borderRadiusLG: 12,
          fontSize: 13,
          controlHeight: 32,
          fontFamily:
            '"Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, Roboto, "Noto Sans", sans-serif',
          colorBgLayout: dark ? "#141518" : "#f4f6f9",
          colorBorderSecondary: dark ? "#2c2e33" : "#e6e9ef",
          ...(dark ? { colorBgContainer: "#1c1d21", colorBgElevated: "#24262b" } : {}),
        },
        components: {
          Table: {
            headerBg: "transparent",
            headerSplitColor: "transparent",
            headerColor: dark ? "#9198a1" : "#656d76",
            cellPaddingBlock: 12,
          },
          Card: { headerFontSize: 14, headerHeight: 48 },
          Button: { fontWeight: 500, primaryShadow: "none", defaultShadow: "none" },
          Segmented: { itemSelectedBg: dark ? "#2c2e33" : "#ffffff" },
        },
      }}
    >
      {/* Thông báo nổi nằm dưới thanh tiêu đề, không đè lên nút cửa sổ */}
      <AntApp message={{ top: 48 }} notification={{ top: 48 }}>
        <App />
      </AntApp>
    </ConfigProvider>
  );
}

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <Root />
  </StrictMode>,
);
