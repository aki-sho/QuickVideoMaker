import { defineConfig } from "vite";

export default defineConfig({
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ["**/src-tauri/target/**", "**/QuickVideoMaker-PortableData/**"],
    },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    outDir: "dist-web",
    target: "chrome105",
    minify: "esbuild",
    sourcemap: false,
  },
});
