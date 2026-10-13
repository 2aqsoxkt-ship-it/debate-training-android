import react from "@vitejs/plugin-react";
import process from "node:process";
import { defineConfig } from "vitest/config";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig(() => ({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  test: {
    exclude: ["**/node_modules/**", "**/tests/ui/**", "**/src-tauri/**"],
    maxWorkers: 2,
    environment: "jsdom",
    setupFiles: "./src/test/setup.ts",
  },
}));
