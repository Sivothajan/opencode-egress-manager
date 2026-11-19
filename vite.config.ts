import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { fileURLToPath, URL } from "node:url";

// Tauri expects a fixed dev port and fails if it is occupied.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          clientPort: 1420,
        }
      : undefined,
    watch: {
      ignored: ["**/src-tauri/**"],
    },
  },
  build: {
    target: "es2022",
    outDir: "dist",
    emptyOutDir: true,
    rolldownOptions: {
      output: {
        // Split the stable vendor libraries out of the app shell so the
        // initial download is smaller and cached across route changes.
        codeSplitting: {
          groups: [
            { name: "react", test: /node_modules[\\/]react[\\/]/ },
            { name: "react-dom", test: /node_modules[\\/]react-dom[\\/]/ },
            { name: "router", test: /node_modules[\\/]react-router[\\/]/ },
            { name: "router-dom", test: /node_modules[\\/]react-router-dom[\\/]/ },
            { name: "query", test: /node_modules[\\/]@tanstack[\\/]/ },
            { name: "state", test: /node_modules[\\/](zustand|zod)[\\/]/ },
            { name: "icons", test: /node_modules[\\/]lucide-react[\\/]/ },
          ],
        },
      },
    },
  },
  test: {
    globals: true,
    environment: "jsdom",
    setupFiles: ["./vitest.setup.ts"],
    css: false,
    include: ["src/**/*.{test,spec}.{ts,tsx}"],
  },
});
