import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The UI is served by the Rust `server` crate from `ui/dist`. In dev mode we proxy the
// server's API (search SSE, tiles, catalog) to the running backend so `vite dev` works
// against a real `cargo run -p server` on :8080.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    proxy: {
      "/api": "http://127.0.0.1:8080",
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});
