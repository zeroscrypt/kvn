import { defineConfig } from "vite";

// Tauri expects a fixed dev port and no auto-clearing of its output.
export default defineConfig({
  clearScreen: false,
  define: {
    __APP_VERSION__: JSON.stringify(process.env.npm_package_version ?? "0.1.0"),
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "es2021",
    sourcemap: false,
  },
});
