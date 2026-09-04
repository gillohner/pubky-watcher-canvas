import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig(({ command }) => ({
  base: command === "build" ? "/watcher-canvas/" : "/",
  plugins: [react()],
  build: {
    // Most of this is the Pubky SDK/WASM bridge used by Ring cookie auth and storage.
    chunkSizeWarningLimit: 3_000,
  },
  server: {
    port: 5173,
    proxy: {
      "/api": "http://127.0.0.1:3001",
    },
  },
}));
