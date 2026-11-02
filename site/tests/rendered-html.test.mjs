import assert from "node:assert/strict";
import test from "node:test";
import { readFile } from "node:fs/promises";

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
  assert.match(html, /<title>vanityctl[^<]*agent[^<]*<\/title>/i);
  assert.match(html, /agent.managed self.hosted hardware/i);
  assert.match(html, /<button[^>]*>[\s\S]*?Copy setup prompt[\s\S]*?<\/button>/);
  assert.match(html, /vanityctl status/);
  assert.match(html, /Codex/);
  assert.match(html, /ThatXliner\/vanityctl/);
  assert.doesNotMatch(html, /Your site is taking shape|Building your site/);
});

test("explains incremental onboarding and links to current documentation", async () => {
  const html = await (await render("/")).text();

  assert.match(html, /inventory/i);
  assert.match(html, /migration/i);
  assert.match(html, /macOS/);
  assert.match(html, /docs\/README\.md/);
  assert.match(html, /docs\/onboarding\.md/);
  assert.doesNotMatch(html, /--all|--host|--not-kubernetes/);
});


test("ships the canonical setup prompt with a selectable no-JavaScript fallback", async () => {
  const html = await (await render("/")).text();
  const prompt = await readFile(new URL("../../prompts/install.md", import.meta.url), "utf8");
  const escaped = prompt.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;").replace(/'/g, "&#x27;");
  assert.ok(html.includes(escaped));
  assert.match(html, /<textarea[^>]*id="setup-prompt-text"[^>]*readonly/i);
  assert.match(html, /<noscript>[\s\S]*Select the text below/);
});
