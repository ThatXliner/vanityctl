import assert from "node:assert/strict";
import test from "node:test";

async function render(path = "/") {
  const workerUrl = new URL("../dist/server/index.js", import.meta.url);
  workerUrl.searchParams.set("test", `${process.pid}-${Date.now()}`);
  const { default: worker } = await import(workerUrl.href);

  return worker.fetch(
    new Request(`https://thatxliner.github.io${path}`, {
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
}

test("server-renders the vanityctl landing page", async () => {
  const response = await render("/");
  assert.equal(response.status, 200);
  assert.match(response.headers.get("content-type") ?? "", /^text\/html\b/i);

  const html = await response.text();
  assert.match(html, /<title>vanityctl — One control plane for this computer<\/title>/i);
  assert.match(html, /Everything this machine is responsible for\./);
  assert.match(html, /vanityctl status/);
  assert.match(html, /Not the smallest Kubernetes\./);
  assert.match(html, /ThatXliner\/vanityctl/);
  assert.doesNotMatch(html, /Your site is taking shape|Building your site/);
});

test("renders the operational model and dogfood proof", async () => {
  const html = await (await render("/")).text();

  for (const label of [
    "Containers",
    "Native processes",
    "Scheduled jobs",
    "Git deployments",
    "YAML registry",
    "11 existing stacks",
    "Zero containers replaced",
  ]) {
    assert.match(html, new RegExp(label));
  }
});
