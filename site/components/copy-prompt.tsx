"use client";

import { useEffect, useRef, useState } from "react";
import { setupPrompt } from "../lib/setup-prompt";

const promptSource = "https://github.com/ThatXliner/vanityctl/blob/main/prompts/install.md";

const statusMessages = {
  idle: "paste it into your coding agent",
  copied: "copied. Paste it into your coding agent.",
  manual:
    "clipboard blocked, so nothing was copied. The full prompt is selected below: press ⌘C or Ctrl+C, or use your device's Copy option.",
};

export function CopyPrompt() {
  const [status, setStatus] = useState<keyof typeof statusMessages>("idle");
  const [expanded, setExpanded] = useState(false);
  const textRef = useRef<HTMLTextAreaElement>(null);

  useEffect(() => {
    const text = textRef.current;
    if (!expanded || !text) return;

    const fitContent = () => {
      text.style.height = "auto";
      text.style.height = `${text.scrollHeight}px`;
    };
    fitContent();
    window.addEventListener("resize", fitContent);
    return () => {
      window.removeEventListener("resize", fitContent);
      text.style.height = "";
    };
  }, [expanded]);

  async function copy() {
    // Clearing first makes a repeat copy announce again.
    setStatus("idle");
    try {
      // navigator.clipboard is undefined on insecure origins, which also lands in catch.
      await navigator.clipboard.writeText(setupPrompt);
      setStatus("copied");
    } catch {
      setExpanded(true);
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
    <section
      id="setup-prompt"
      className="prompt-panel"
      aria-labelledby="setup-prompt-label"
      data-expanded={expanded}
    >
      <div className="prompt-head">
        <button type="button" className="button" onClick={copy}>
          Copy setup prompt
        </button>
        <p id="setup-prompt-status" className="prompt-status" role="status" data-status={status}>
          {statusMessages[status]}
        </p>
        <noscript>
          <p className="prompt-status" data-status="manual">
            JavaScript is off, so the button cannot copy. Select the text below and copy it yourself.
          </p>
        </noscript>
        <label id="setup-prompt-label" className="prompt-label" htmlFor="setup-prompt-text">
          Setup prompt
        </label>
      </div>
      <textarea
        ref={textRef}
        id="setup-prompt-text"
        className="prompt-text"
        value={setupPrompt}
        rows={5}
        readOnly
        spellCheck={false}
        aria-describedby="setup-prompt-status"
      />
      <div className="prompt-foot">
        <button
          type="button"
          className="text-button bracketed"
          aria-expanded={expanded}
          aria-controls="setup-prompt-text"
          onClick={() => setExpanded(!expanded)}
        >
          {expanded ? "collapse prompt" : "show full prompt"}
        </button>
        <a className="text-link" href={promptSource}>
          prompts/install.md ↗
        </a>
      </div>
    </section>
  );
}
