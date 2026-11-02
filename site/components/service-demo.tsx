"use client";

import { useRef, useState, type KeyboardEvent, type ReactNode } from "react";

// Examples show selected fields from the CLI JSON, alongside the real registry schema.
const steps = [
  {
    id: "discover",
    comment: "# find what is already here",
    command: "vanityctl discover my-app --json",
    lang: "json",
    output: `{
  "resources": [{
    "name": "my-app",
    "kind": "compose",
    "managedAs": []
  }]
}`,
  },
  {
    id: "register",
    comment: "# register; no restart",
    command: "cat ~/.vanityctl/services/my-app.yaml",
    lang: "yaml",
    output: `services:
  my-app:
    type: compose
    directory: ~/services/my-app
    files:
      - compose.yaml`,
  },
  {
    id: "context",
    comment: "# next session",
    command: "vanityctl agent-context my-app --json",
    lang: "json",
    output: `{
  "services": [{
    "name": "my-app",
    "type": "compose",
    "files": ["compose.yaml"]
  }]
}`,
    next: "vanityctl up my-app",
  },
] as const;

const tokenPatterns = {
  json: /("(?:[^"\\]|\\.)*")(?=:)|("(?:[^"\\]|\\.)*")|\b(null|true|false|\d+)\b/g,
  yaml: /^(\s*)([\w-]+)(?=:)/gm,
};

function highlight(text: string, lang: keyof typeof tokenPatterns) {
  const nodes: ReactNode[] = [];
  let last = 0;
  for (const match of text.matchAll(tokenPatterns[lang])) {
    const start = match.index ?? 0;
    nodes.push(text.slice(last, start));
    if (lang === "yaml") {
      nodes.push(match[1], <span className="tok-key" key={start}>{match[2]}</span>);
    } else {
      const kind = match[1] ? "tok-key" : match[2] ? "tok-string" : "tok-literal";
      nodes.push(<span className={kind} key={start}>{match[0]}</span>);
    }
    last = start + match[0].length;
  }
  nodes.push(text.slice(last));
  return nodes;
}

export function ServiceDemo() {
  const [active, setActive] = useState(0);
  const tabRefs = useRef<(HTMLButtonElement | null)[]>([]);

  function onKeyDown(event: KeyboardEvent) {
    const last = steps.length - 1;
    const keys: Record<string, number> = { ArrowRight: active === last ? 0 : active + 1, ArrowLeft: active === 0 ? last : active - 1, Home: 0, End: last };
    const next = keys[event.key];
    if (next === undefined) return;
    event.preventDefault();
    setActive(next);
    tabRefs.current[next]?.focus();
  }

  return (
    <div className="demo-window" data-ui>
      <div className="demo-bar">
        <div className="demo-tabs" role="tablist" aria-label="Example session steps">
          {steps.map((step, i) => (
            <button
              key={step.id}
              ref={(node) => { tabRefs.current[i] = node; }}
              id={`demo-tab-${step.id}`}
              type="button"
              role="tab"
              aria-selected={active === i}
              aria-controls={`demo-panel-${step.id}`}
              tabIndex={active === i ? 0 : -1}
              onClick={() => setActive(i)}
              onKeyDown={onKeyDown}
            >
              {step.id}
            </button>
          ))}
        </div>
        <span className="demo-label">example excerpt</span>
      </div>
      {steps.map((step, i) => (
        <div
          key={step.id}
          id={`demo-panel-${step.id}`}
          className="demo-panel"
          role="tabpanel"
          aria-labelledby={`demo-tab-${step.id}`}
          tabIndex={0}
          hidden={active !== i}
        >
          <pre>
            <span className="tok-comment">{step.comment}</span>{"\n"}
            <span className="demo-command"><span className="demo-prompt" aria-hidden="true">$ </span><span className="typed">{step.command}</span></span>{"\n"}
            <span className="demo-output">{highlight(step.output, step.lang)}</span>{"\n"}
            <span className="demo-command"><span className="demo-prompt" aria-hidden="true">$ </span>{"next" in step ? step.next : null}<span className="cursor" aria-hidden="true" /></span>
          </pre>
        </div>
      ))}
    </div>
  );
}
