import { StrictMode, useEffect, useState } from "react";
import { createRoot } from "react-dom/client";
import { App as AntApp, ConfigProvider, theme } from "antd";
import viVN from "antd/locale/vi_VN";
import dayjs from "dayjs";
import "dayjs/locale/vi";
import relativeTime from "dayjs/plugin/relativeTime";
import App from "./App";
import "./styles.css";

dayjs.extend(relativeTime);
dayjs.locale("vi");

/** Theo chế độ sáng/tối của hệ điều hành. */
function useSystemDark() {
  const mq = window.matchMedia("(prefers-color-scheme: dark)");
  const [dark, setDark] = useState(mq.matches);
  useEffect(() => {
    const on = (e: MediaQueryListEvent) => setDark(e.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, [mq]);
  return dark;
}

function Root() {
  const dark = useSystemDark();
  useEffect(() => {
    document.documentElement.dataset.theme = dark ? "dark" : "light";
  }, [dark]);
  return (
    <ConfigProvider
      locale={viVN}
      theme={{
        algorithm: dark ? theme.darkAlgorithm : theme.defaultAlgorithm,
        token: {
          colorPrimary: "#1f6feb",
          borderRadius: 8,
          fontFamily:
            'system-ui, -apple-system, "Segoe UI", Roboto, "Noto Sans", "Helvetica Neue", sans-serif',
        },
        components: {
          Layout: { siderBg: dark ? "#141414" : "#ffffff", headerBg: "transparent" },
          Menu: { itemBg: "transparent" },
        },
      }}
    >
      <AntApp>
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
