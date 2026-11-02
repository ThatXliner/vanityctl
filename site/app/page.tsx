import { CopyPrompt } from "../components/copy-prompt";

const github = "https://github.com/ThatXliner/vanityctl";
const docs = `${github}/blob/main/docs`;

function Prompt({ children }: { children?: React.ReactNode }) {
  return <div className="prompt"><span className="prompt-host">big-mac</span><span className="prompt-path">~</span><span className="prompt-mark" aria-hidden="true">❯</span><span>{children}</span></div>;
}

export default function Home() {
  return (
    <main>
      <div className="terminal-window">
        <header className="titlebar">
          <div className="traffic" aria-hidden="true"><i /><i /><i /></div>
          <span>big-mac — vanityctl<span className="window-size"> — 120×40</span></span>
        </header>

        <nav aria-label="Documentation">
          <a href={`${docs}/onboarding.md`}>onboarding</a>
          <a href={`${docs}/README.md`}>docs</a>
          <a href={github}>github ↗</a>
        </nav>

        <div className="session">
          <section className="hero output-block">
            <p className="comment"># one registry and CLI for the services on your machine</p>
            <h1><span>Agent-managed</span> <span>self-hosted hardware.</span></h1>
            <p className="lede">Give Codex, Claude Code or your coding agent one prompt. One registry, one CLI, fewer “wait, what’s running on this thing?” sessions.</p>
          </section>

          <div className="setup">
            <Prompt>cat prompts/install.md</Prompt>
            <CopyPrompt />
          </div>

          <footer>
            <Prompt><span className="cursor" aria-hidden="true" /></Prompt>
            <p>MIT licensed · Rust · macOS first</p>
          </footer>
        </div>
      </div>
    </main>
  );
}
