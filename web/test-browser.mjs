import { chromium } from "playwright";
import assert from "node:assert/strict";
import { spawn, execFileSync } from "node:child_process";
import { once } from "node:events";
import { createInterface } from "node:readline";
import {
  cp,
  readFile,
  symlink,
  mkdtemp,
  writeFile,
  rm,
  mkdir,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("..", import.meta.url));
const development = process.argv.includes("--dev");
const binary = resolve(
  root,
  process.env.AMITOKI_BINARY ?? "target/release/amitoki",
);
const directory = await mkdtemp(join(tmpdir(), "amitoki-web-test-"));
const artifacts = resolve(
  root,
  process.env.AMITOKI_WEB_ARTIFACTS ?? "artifacts/web",
);
await mkdir(artifacts, { recursive: true });
const store = join(directory, "plugins");
for (const plugin of ["telemetry", "telemetry-rewrite"])
  execFileSync(binary, [
    "plugin",
    "--directory",
    store,
    "stage",
    "add",
    join(root, "dist", plugin),
  ]);
const config = join(directory, "app.toml");
await writeFile(
  config,
  `node_id='lab-a'
channel='telemetry-lab'
interface='unused'
[firewall]
policy='blacklist'
[[pipeline.relays]]
id='sink'
plugin='memory'
[[pipeline.stages]]
id='decode'
plugin='telemetry'
[[pipeline.stages]]
id='filter'
plugin='telemetry'
[pipeline.stages.options]
operation='filter'
threshold=80
[[pipeline.stages]]
id='rewrite'
plugin='telemetry-rewrite'
[pipeline.stages.options]
temperature=25
redact_payload=true
[[pipeline.routes]]
from='capture'
to=['decode']
[[pipeline.routes]]
from='decode.pass'
to=['filter']
[[pipeline.routes]]
from='decode.drop'
to=[]
[[pipeline.routes]]
from='filter.pass'
to=['rewrite']
[[pipeline.routes]]
from='filter.drop'
to=[]
[[pipeline.routes]]
from='rewrite.pass'
to=['sink']
[[pipeline.routes]]
from='rewrite.drop'
to=[]
[[pipeline.routes]]
from='sink.received'
to=['inject']
`,
);
const header = Buffer.alloc(24);
header.writeUInt32LE(0xa1b2c3d4);
header.writeUInt16LE(2, 4);
header.writeUInt16LE(4, 6);
header.writeUInt32LE(65535, 16);
header.writeUInt32LE(1, 20);
const records = [22, 99, 33].map((temperature, index) => {
  const packet = Buffer.alloc(60);
  Buffer.from("02000000002002000000001088b5414d544b0100", "hex").copy(packet);
  packet.writeBigUInt64BE(BigInt(index + 1), 20);
  packet.writeInt16BE(temperature, 28);
  packet.writeUInt16BE(3, 30);
  packet.write("abc", 32);
  const record = Buffer.alloc(16);
  record.writeUInt32LE(index, 0);
  record.writeUInt32LE(packet.length, 8);
  record.writeUInt32LE(packet.length, 12);
  return Buffer.concat([record, packet]);
});
const capture = Buffer.concat([header, ...records]);
await writeFile(join(directory, "input.pcap"), capture);
const child = spawn(
  binary,
  ["web", "--config", config, "--directory", store, "--listen", "127.0.0.1:0"],
  { stdio: ["ignore", "pipe", "inherit"] },
);
let browser;
let devServer;
const frontend = join(directory, "web");
try {
  const lines = createInterface({ input: child.stdout });
  const [url] = await Promise.race([
    once(lines, "line", { signal: AbortSignal.timeout(10000) }),
    once(child, "exit").then(() => {
      throw new Error("Webの起動に失敗");
    }),
  ]);
  let frontendUrl = url;
  if (development) {
    process.env.AMITOKI_WEB_BACKEND = new URL(url).origin;
    await mkdir(frontend);
    // HMR検証は一時コピーだけを編集し、作業中のソースへ書き込まない。
    for (const name of [
      "src",
      "public",
      "index.html",
      "vite.config.ts",
      "package.json",
    ]) {
      await cp(join(root, "web", name), join(frontend, name), {
        recursive: true,
      });
    }
    await symlink(
      join(root, "web", "node_modules"),
      join(frontend, "node_modules"),
      "dir",
    );
    const { createServer } = await import("vite");
    devServer = await createServer({
      root: frontend,
      server: { port: 0 },
    });
    await devServer.listen();
    frontendUrl = `${devServer.resolvedUrls.local[0]}${new URL(url).hash}`;
  }
  browser = await chromium.launch({
    headless: true,
    ...(process.env.AMITOKI_CHROMIUM
      ? { executablePath: process.env.AMITOKI_CHROMIUM }
      : {}),
  });
  const page = await browser.newPage({
    viewport: { width: 1100, height: 950 },
    colorScheme: "dark",
  });
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => {
    if (message.type() === "error") errors.push(message.text());
  });
  await page.goto(frontendUrl);
  if (development)
    assert.equal(await page.locator('script[src="/@vite/client"]').count(), 1);
  await page.locator("#pcap").waitFor();
  const uploaded = page.waitForResponse(
    (response) =>
      response.url().includes("/api/capture?") &&
      response.request().method() === "POST",
    { timeout: 125000 },
  );
  await page.locator("#pcap").setInputFiles(join(directory, "input.pcap"));
  const response = await uploaded;
  assert.equal(response.status(), 200);
  await page.locator('[data-packet="1"]').waitFor();
  const token = await page.evaluate(() =>
    sessionStorage.getItem("amitoki-token"),
  );
  const origin = new URL(frontendUrl).origin;
  const loaded = await page.request.get(`${origin}/api/capture`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  const dataset = await loaded.json();
  assert.equal(dataset.packets.length, 3);
  assert.deepEqual(dataset.packets[1].terminals, []);
  assert.notEqual(
    dataset.packets[0].steps[2].input.hex,
    dataset.packets[0].steps[2].output.hex,
  );
  await page.locator('[data-step="2"]').click();
  assert.equal(await page.locator(".am-diff-panel").count(), 2);
  assert.ok((await page.locator("mark").count()) > 0);
  if (development) {
    const component = join(frontend, "src", "packets", "SnapshotPanel.tsx");
    const original = await readFile(component, "utf8");
    await page.evaluate(() => {
      window.amitokiHmrVerified = true;
    });
    await writeFile(
      component,
      original.replaceAll(
        'className="am-diff-panel"',
        'data-hmr-verified="true" className="am-diff-panel"',
      ),
    );
    await page.locator('[data-hmr-verified="true"]').first().waitFor();
    assert.equal(await page.evaluate(() => window.amitokiHmrVerified), true);
    assert.equal(
      await page.locator('[data-step="2"]').getAttribute("aria-pressed"),
      "true",
    );
    await writeFile(component, original);
    await page.waitForFunction(
      () => !document.querySelector("[data-hmr-verified]"),
    );
    const foreign = await page.request.get(`${origin}/api/topology`, {
      headers: {
        Authorization: `Bearer ${token}`,
        Origin: "https://attacker.example",
      },
    });
    assert.equal(foreign.status(), 403);
  }
  await page.locator('[data-packet="2"]').click();
  await page.locator('[data-step="1"]').click();
  assert.ok(
    (await page.locator(".am-annotation").textContent()).includes("送信先なし"),
  );
  const layouts = [];
  for (const width of [1100, 1024, 768, 375, 320]) {
    await page.setViewportSize({ width, height: 950 });
    for (const view of ["pipeline", "packets", "operations"]) {
      await page.locator(`.am-nav [data-view="${view}"]`).click();
      const geometry = await page
        .locator("#amitoki-design")
        .evaluate((element) => ({
          width: element.clientWidth,
          scroll: element.scrollWidth,
          bodyWidth: document.body.clientWidth,
          bodyScroll: document.body.scrollWidth,
          scheme: getComputedStyle(element).colorScheme,
          header: element.querySelector(".am-top").getBoundingClientRect()
            .height,
        }));
      assert.ok(
        geometry.scroll <= geometry.width + 2 &&
          geometry.bodyScroll <= geometry.bodyWidth + 2,
        JSON.stringify({ width, view, geometry }),
      );
      assert.equal(geometry.scheme, "light");
      assert.ok(geometry.header < 45);
      if (view === "pipeline")
        assert.ok((await page.locator(".am-graph-edges path").count()) > 0);
      layouts.push({ width, view, ...geometry });
    }
  }
  await page.setViewportSize({ width: 1100, height: 950 });
  await page.locator('.am-nav [data-view="packets"]').click();
  await page.locator('[data-packet="1"]').click();
  await page.locator('[data-step="2"]').click();
  await page.screenshot({
    path: join(artifacts, "packets.png"),
    fullPage: true,
  });
  // 破損PCAPでも現在の結果を失わない。
  const invalid = await page.request.post(
    `${origin}/api/capture?name=bad.pcap`,
    {
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/octet-stream",
      },
      data: "invalid",
    },
  );
  assert.equal(invalid.status(), 422);
  const preserved = await page.request.get(`${origin}/api/capture`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  assert.equal((await preserved.json()).name, "input.pcap");
  const unsafeName = '<img src=x onerror="window.fileNameExecuted=true">.pcap';
  const named = await page.request.post(
    `${origin}/api/capture?${new URLSearchParams({ name: unsafeName })}`,
    {
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/octet-stream",
      },
      data: capture,
      timeout: 125000,
    },
  );
  assert.equal(named.status(), 200);
  await page.reload();
  await page.locator('[data-packet="1"]').waitFor();
  assert.equal(await page.locator("h2").textContent(), unsafeName);
  assert.equal(await page.locator("#amitoki-design img").count(), 0);
  assert.equal(await page.evaluate(() => window.fileNameExecuted), undefined);
  // 状態取得のHTTP経路はRustで実測し、画面では接続・切断時の切替を再現する。
  const topologyResponse = await page.request.get(`${origin}/api/topology`, {
    headers: { Authorization: `Bearer ${token}` },
  });
  const topology = await topologyResponse.json();
  let live = {
    running: true,
    status: {
      topology,
      generation: 1,
      captured: 3,
      published: 2,
      injected: 0,
      filtered: 1,
      rejected: 0,
      retries: 0,
      relays: [],
    },
  };
  await page.route("**/api/status", (route) => route.fulfill({ json: live }));
  await page.waitForFunction(
    () => document.getElementById("connection-state").textContent === "稼働中",
  );
  await page.locator('.am-nav [data-view="pipeline"]').click();
  await page.locator("#topology-source").selectOption("active");
  await page.locator("#topology-source").focus();
  await page.waitForResponse((response) =>
    response.url().endsWith("/api/status"),
  );
  assert.equal(
    await page.evaluate(() => document.activeElement.id),
    "topology-source",
  );
  live = { running: false, status: null };
  await page.waitForFunction(
    () => document.getElementById("connection-state").textContent === "未取得",
  );
  assert.equal(await page.locator("#topology-source").inputValue(), "replay");
  assert.equal(errors.length, 0, errors.join("\n"));
  await writeFile(
    join(artifacts, "verification.json"),
    JSON.stringify(
      {
        status: "passed",
        mode: development ? "development" : "production",
        hmr: development,
        layouts,
        errors,
        packets: dataset.packets.length,
      },
      null,
      2,
    ),
  );
  console.log(
    "Web UI: PCAP・加工/破棄・構成図・5幅×3画面・白テーマ・状態復元を確認",
  );
} finally {
  await browser?.close();
  await devServer?.close();
  if (child.exitCode === null) {
    const exited = once(child, "exit");
    child.kill("SIGTERM");
    await exited;
  }
  await rm(directory, { recursive: true, force: true });
}
