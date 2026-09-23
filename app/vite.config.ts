import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  server: {
    port: 5320,
    // `pnpm dev` has no Pages functions, so stand in for /api/blocks. The real
    // thing caches and falls back; this only forwards.
    proxy: {
      "/api/blocks": {
        target: "https://api.blockchair.com",
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/api\/blocks/, "/zcash/blocks"),
      },
    },
  },
});
