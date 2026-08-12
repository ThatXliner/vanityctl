import { cp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const out = resolve(root, "out");
const workerUrl = new URL("../dist/server/index.js", import.meta.url);
workerUrl.searchParams.set("export", `${Date.now()}`);
const { default: worker } = await import(workerUrl.href);

const response = await worker.fetch(
  new Request("https://thatxliner.github.io/", {
    headers: { accept: "text/html" },
  }),
  {
    ASSETS: {
      fetch: async () => new Response("Not found", { status: 404 }),
    },
  },
  {
    waitUntil() {},
    passThroughOnException() {},
  },
);

if (!response.ok) {
  throw new Error(`Unable to render landing page: HTTP ${response.status}`);
}

await rm(out, { recursive: true, force: true });
await mkdir(out, { recursive: true });
await cp(resolve(root, "dist/client"), out, { recursive: true });
const html = (await response.text()).replaceAll(
  'href="/_next/',
  'href="/vanityctl/_next/',
).replaceAll('src="/_next/', 'src="/vanityctl/_next/')
  .replaceAll('url(/_next/', 'url(/vanityctl/_next/');
await writeFile(resolve(out, "index.html"), html);
await writeFile(resolve(out, ".nojekyll"), "");

const exported = await readFile(resolve(out, "index.html"), "utf8");
if (!exported.includes("Everything this machine")) {
  throw new Error("Static export did not contain the vanityctl landing page");
}

console.log(`Exported GitHub Pages artifact to ${out}`);
