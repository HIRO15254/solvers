import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The GUI is served by `solversd --ui web/dist` from the daemon's own origin.
// In development the Vite server proxies the API to a daemon on the default
// address; set SOLVERSD_URL to point it elsewhere.
const daemon = process.env.SOLVERSD_URL ?? "http://127.0.0.1:38127";

export default defineConfig({
  plugins: [react()],
  base: "./",
  server: {
    proxy: { "/v1": { target: daemon, changeOrigin: true } },
    fs: { allow: [".."] },
  },
  build: { outDir: "dist", emptyOutDir: true },
});
