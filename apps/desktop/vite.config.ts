import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Cấu hình chuẩn cho Tauri: cổng cố định, không clear terminal
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  // App desktop chạy local: bundle antd ~400 KB gzip không ảnh hưởng tốc độ tải
  build: { target: "es2022", outDir: "dist", chunkSizeWarningLimit: 1600 },
});
