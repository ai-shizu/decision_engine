import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri mobile (`ios`/`android` dev) sets TAURI_DEV_HOST to the host LAN IP.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig(async () => ({
  plugins: [react()],
  // Tauri 本番ビルドでは相対パス必須 (絶対パスだと黒画面になる)
  base: "./",
  clearScreen: false,
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target:
      process.env.TAURI_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: process.env.TAURI_DEBUG ? false : "esbuild",
    sourcemap: !!process.env.TAURI_DEBUG,
  },
  server: {
    port: 1420,
    strictPort: true,
    // Always bind 0.0.0.0 so iOS Simulator / device can reach the Mac Vite.
    // (host:false → loopback-only → WKWebView black screen on mobile.)
    host: true,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : {
          protocol: "ws",
          host: "localhost",
          port: 1421,
        },
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
}));
