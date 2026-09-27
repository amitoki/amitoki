import { createServer } from "vite";
import { fileURLToPath } from "node:url";

// ポート0も実際のlisten結果から返し、APIの認証URLはPython側で組み立てる。
const server = await createServer({
  root: fileURLToPath(new URL(".", import.meta.url)),
  server: {
    port: Number(process.argv[2]),
    host: "127.0.0.1",
    strictPort: true,
  },
});
await server.listen();
console.log(`AMITOKI_VITE_READY=${server.resolvedUrls.local[0]}`);

for (const name of ["SIGINT", "SIGTERM"]) {
  process.once(name, async () => {
    await server.close();
    process.exit(0);
  });
}
