import { CopyPrompt } from "../components/copy-prompt";

const github = "https://github.com/ThatXliner/vanityctl";
const docs = `${github}/blob/main/docs`;

export default function Home() {
  return (
    <div className="page">
      <header className="site-header">
        <nav aria-label="Project">
          <a href={`${docs}/README.md`}>Docs</a>
          <a href={`${docs}/onboarding.md`}>Onboarding</a>
          <a href={github}>GitHub</a>
        </nav>
      </header>

      <main>
        <section className="hero">
          <h1>vanityctl</h1>
          <p className="tagline">Agent-managed self-hosted hardware.</p>
          <p className="intro">Give Codex, Claude Code or your agent one prompt to bring existing services into one registry and CLI.</p>
          <CopyPrompt />
        </section>

        <div className="details">
          <section aria-labelledby="runs">
            <h2 id="runs">What it runs</h2>
            <ul>
              <li>Docker containers and existing Compose stacks</li>
              <li>Native processes and scheduled jobs, through launchd</li>
              <li>Git deployments and Cloudflare DNS, if you want them</li>
            </ul>
            <p className="note">macOS first. Linux native services and systemd are not implemented yet.</p>
          </section>

          <section aria-labelledby="context">
            <h2 id="context">Context that carries over</h2>
            <p>Setup starts with a read-only inventory and a migration plan. Services move over gradually, each with a health check and a rollback, and anything unsupported stays with its current owner.</p>
            <p>What the agent learns stays in the registry. Next session it starts from <code>vanityctl status</code> or <code>vanityctl agent-context NAME --json</code> instead of digging through Compose files and plists again.</p>
            <p>A local daemon does the actual work, the same way every time. No hosted service, no LLM inside.</p>
            <p className="links">
              <a href={`${docs}/onboarding.md`}>Onboarding guide</a>
              <a href={`${docs}/README.md`}>Documentation</a>
            </p>
          </section>
        </div>
      </main>

      <footer className="site-footer">
        <p>MIT licensed · Written in Rust</p>
        <a href={github}>github.com/ThatXliner/vanityctl</a>
      </footer>
    </div>
  );
}
