import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig(async () => ({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_"],
  build: {
    target: process.env.TAURI_PLATFORM === "windows" ? "chrome105" : "safari13",
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_DEBUG,
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) return undefined;
          if (id.includes("/react/") || id.includes("/react-dom/") || id.includes("/scheduler/")) {
            return "vendor-react";
          }
          if (id.includes("/recharts/") || id.includes("/d3-")) {
            return "vendor-charts";
          }
          if (id.includes("/xlsx/")) {
            return "vendor-xlsx";
          }
          if (id.includes("/@tauri-apps/")) {
            return "vendor-tauri";
          }
          if (id.includes("/lucide-react/")) {
            return "vendor-icons";
          }
          return "vendor";
        },
      },
    },
  },
}));
