import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  // Alt-yolda (/ui/) sunulduğu için göreli asset yolları şart
  base: "./",
  plugins: [react()],
  server: {
    port: 3000,
    proxy: {
      // Üretimdeki Nginx gibi /api önekini sıyırır (/api/plugins → /plugins); API rotaları öneksiz.
      "/api": {
        target: "http://127.0.0.1:8001",
        changeOrigin: true,
        rewrite: (path) => path.replace(/^\/api/, ""),
      },
    },
  },
});
