"use client";

import { useRef, useState } from "react";
import { setupPrompt } from "../lib/setup-prompt";

const statusMessages = {
  idle: "Then paste it into your coding agent.",
  copied: "Copied. Paste it into your coding agent.",
  manual:
    "Your browser blocked copying, so nothing was copied. The full prompt is selected below: press ⌘C or Ctrl+C, or use your device's Copy option.",
};

export function CopyPrompt() {
  const [status, setStatus] = useState<keyof typeof statusMessages>("idle");
  const [expanded, setExpanded] = useState(false);
  const detailsRef = useRef<HTMLDetailsElement>(null);
  const textRef = useRef<HTMLTextAreaElement>(null);

  async function copy() {
    // Clearing first makes a repeat copy announce again.
    setStatus("idle");
    try {
      // navigator.clipboard is undefined on insecure origins, which also lands in catch.
      await navigator.clipboard.writeText(setupPrompt);
      setStatus("copied");
    } catch {
      // Open synchronously so the textarea is focusable right away.
      if (detailsRef.current) detailsRef.current.open = true;
      const text = textRef.current;
      if (text) {
        text.focus();
        text.select();
        text.setSelectionRange(0, text.value.length);
      }
      setStatus("manual");
    }
  }

  return (
    <section id="setup-prompt" className="prompt-panel" aria-label="Setup prompt" data-expanded={expanded}>
      <div className="prompt-head">
        <button type="button" className="button" onClick={copy}>
          Copy setup prompt
        </button>
        <p id="setup-prompt-status" className="prompt-status" role="status" data-status={status}>
          {statusMessages[status]}
        </p>
        <noscript>
          <p className="prompt-status" data-status="manual">
            Copying needs JavaScript. Select the text below, under Read prompt, and copy it yourself.
          </p>
        </noscript>
        <details
          ref={detailsRef}
          className="prompt-details"
          onToggle={(event) => setExpanded(event.currentTarget.open)}
        >
          <summary>Read prompt</summary>
          <label className="visually-hidden" htmlFor="setup-prompt-text">
            Full setup prompt
          </label>
          <textarea
            ref={textRef}
            id="setup-prompt-text"
            className="prompt-text"
            value={setupPrompt}
            rows={16}
            readOnly
            spellCheck={false}
            aria-describedby="setup-prompt-status"
          />
        </details>
      </div>
    </section>
  );
}
