import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// 開発時だけViteから同じマシンのAPIへ中継する。トークンはブラウザから送る。
const backend = new URL(
  process.env.AMITOKI_WEB_BACKEND ?? "http://127.0.0.1:8710",
);
if (
  backend.protocol !== "http:" ||
  !["127.0.0.1", "[::1]"].includes(backend.hostname)
) {
  throw new Error(
    "AMITOKI_WEB_BACKENDにはループバックのHTTP URLを指定してください",
  );
}

export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
    cors: false,
    proxy: {
      "/api": {
        target: backend.origin,
        changeOrigin: true,
        configure(proxy) {
          proxy.on("proxyReq", (outgoing, incoming) => {
            // Vite自身への同一Origin要求だけ書き換え、第三者Originは本体で拒否する。
            if (incoming.headers.origin === `http://${incoming.headers.host}`) {
              outgoing.setHeader("Origin", backend.origin);
            }
          });
        },
      },
    },
  },
});
