import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Cấu hình chuẩn cho Tauri: cổng cố định, không clear terminal
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 1420, strictPort: true },
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: { target: "es2022", outDir: "dist" },
});
