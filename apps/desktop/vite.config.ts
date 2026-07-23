import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri mobile (`ios`/`android` physical-device) sets TAURI_DEV_HOST to the
// host LAN IP (or USB TUN). Without binding that host, WKWebView paints black.
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
    // Physical iOS: bind the TAURI_DEV_HOST interface. Otherwise always
    // 0.0.0.0 so Simulator / LAN clients can reach the Mac Vite.
    // (host:false → loopback-only → WKWebView black screen on mobile.)
    host: host || true,
    // Vite 7 host-check: allow the injected LAN/TUN host explicitly.
    allowedHosts: true,
    hmr: host
      ? {
          // Same port as the page origin so iOS `devCsp` `'self'` covers the
          // WebSocket (separate :1421 is a different origin and gets blocked).
          protocol: "ws",
          host,
          port: 1420,
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
