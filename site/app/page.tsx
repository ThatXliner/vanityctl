import { CopyPrompt } from "../components/copy-prompt";

const github = "https://github.com/ThatXliner/vanityctl";
const docs = `${github}/blob/main/docs`;

const stages = [
  {
    title: "A read-only inventory",
    text: "The OS, installed tools, Docker and Compose workloads, native services, scheduled jobs, startup owners, deployers, ports and persistent data. Credentials and environment values stay private.",
  },
  {
    title: "A migration plan you can read",
    text: "Each service gets its current owner, proposed registry entry, dependencies, health check and rollback. Registering a service is listed apart from handoffs and restarts, and you approve any downtime you have not already authorized.",
  },
  {
    title: "A gradual move",
    text: "Compose stacks keep their project names, file order, image pins and volumes. A launchd handoff is previewed before it runs. Whatever is unsupported stays with its current owner, with the reason written down.",
  },
  {
    title: "A check against the real app",
    text: "Each changed service is tested locally and through its normal public route, because a running process can still be a broken app. When a migration fails, the agent stops and rolls that service back before continuing with it.",
  },
];

const commands = [
  {
    command: "vanityctl discover QUERY --json",
    text: "Finds existing Docker, Compose and launchd resources, stopped ones included. It only reads, and it adopts nothing.",
  },
  {
    command: "vanityctl agent-context NAME --json",
    text: "Returns one service's type, directory, Compose file order, deployment setup and next commands. Environment values are left out.",
  },
  {
    command: "vanityctl status NAME --json",
    text: "Reports that service's current state by probing only its backend, so unrelated stacks are not polled.",
  },
  {
    command: "vanityctl up NAME",
    text: "Reconciles exactly that one service. Everything else on the machine is left alone.",
  },
];

const coverage = [
  {
    title: "Docker and Compose",
    text: "Docker containers and your existing Compose stacks, registered with the files and project names they already use.",
  },
  {
    title: "launchd processes and jobs",
    text: "Native processes and scheduled jobs on macOS. Supported current-user LaunchAgents can be adopted after a preview.",
  },
  {
    title: "Git deploys and DNS, optionally",
    text: "Git deployment and Cloudflare DNS reconciliation when you want them. A deployer you already run can stay in charge.",
  },
  {
    title: "Not yet",
    text: "Linux native-service management and systemd are not implemented. Standalone containers have no automatic adoption command.",
  },
];

const footerLinks = [
  ["Documentation", `${docs}/README.md`],
  ["Onboarding", `${docs}/onboarding.md`],
  ["Agent workflows", `${docs}/agent-workflows.md`],
  ["How it compares", `${docs}/comparison.md`],
  ["GitHub", github],
  ["MIT license", `${github}/blob/main/LICENSE`],
];

export default function Home() {
  return (
    <>
      <a className="skip-link" href="#setup-prompt">Skip to the setup prompt</a>

      <header id="top" className="site-header">
        <a className="wordmark" href="#top"><span aria-hidden="true">❯ </span>vanityctl</a>
        <nav aria-label="Main">
          <a className="nav-section" href="#plan">Plan</a>
          <a className="nav-section" href="#registry">Registry</a>
          <a className="nav-section" href="#cli">CLI</a>
          <a className="nav-section" href="#coverage">Coverage</a>
          <a href={`${docs}/README.md`}>Docs</a>
          <a href={github}>GitHub</a>
        </nav>
      </header>

      <main>
        <section className="hero">
          <h1>
            <span>Agent-managed</span> <span>self-hosted hardware</span>
          </h1>
          <div className="hero-copy">
            <p className="statement">Your coding agent already writes your software. With this prompt, it can look after the machine that runs it.</p>
            <p>vanityctl gives Codex, Claude Code and other coding agents a lasting record of your host and one CLI to operate it.</p>
          </div>
          <CopyPrompt />
          <div className="hero-links">
            <a className="text-link" href={`${docs}/README.md`}>Read the docs</a>
            <a className="text-link" href={github}>View on GitHub</a>
          </div>
        </section>

        <section id="plan" className="band plan">
          <div className="plan-intro reveal">
            <h2>Nothing moves before the plan</h2>
            <p>The setup prompt tells your agent to work in this order, so you see a description of your machine and a plan before any workload changes.</p>
            <a className="text-link" href={`${docs}/onboarding.md`}>Read the onboarding guide</a>
          </div>
          <ol className="stages">
            {stages.map((stage) => (
              <li className="reveal" key={stage.title}>
                <h3>{stage.title}</h3>
                <p>{stage.text}</p>
              </li>
            ))}
          </ol>
        </section>

        <section id="registry" className="band registry">
          <figure className="reveal">
            <pre data-ui><code><span className="code-note"># ~/.vanityctl/services/my-app.yaml</span>{`\n`}<span className="code-key">services:</span>{`\n`}  <span className="code-name">my-app:</span>{`\n`}    <span className="code-key">type:</span> compose{`\n`}    <span className="code-key">directory:</span> ~/services/my-app{`\n`}    <span className="code-key">files:</span>{`\n`}      - compose.yaml{`\n`}      - compose.production.yaml</code></pre>
            <figcaption>An existing Compose stack, registered. Writing this down adds knowledge without recreating its containers.</figcaption>
          </figure>
          <div className="registry-copy reveal">
            <h2>The next agent starts informed</h2>
            <p>Without a registry, every new session digs through plists, Compose files and startup scripts to work out what runs where. With one, the agent asks vanityctl.</p>
            <p>Setup leaves behind that registry and an infrastructure repo holding health checks, recovery pointers and ownership exceptions. Secret values stay out of public Git, and each session adds to the same record.</p>
          </div>
        </section>

        <section id="cli" className="band cli">
          <div className="cli-intro reveal">
            <h2>The CLI does the work</h2>
            <p>Your agent decides what should happen. vanityctl carries it out through a local daemon, with the same commands and backends each time. It needs no hosted agent service and has no LLM inside, and you can run every command yourself.</p>
          </div>
          <dl className="commands" data-ui>
            {commands.map((item) => (
              <div className="reveal" key={item.command}>
                <dt><code>{item.command}</code></dt>
                <dd>{item.text}</dd>
              </div>
            ))}
          </dl>
        </section>

        <section id="coverage" className="band coverage">
          <h2 className="reveal">What it runs today</h2>
          <div className="coverage-list">
            {coverage.map((item) => (
              <div className="reveal" key={item.title}>
                <h3>{item.title}</h3>
                <p>{item.text}</p>
              </div>
            ))}
          </div>
          <p className="coverage-links reveal">
            <a className="text-link" href={`${docs}/guide.md#project-status-and-limitations`}>Current limitations</a>
            <a className="text-link" href={`${docs}/comparison.md`}>How it compares</a>
          </p>
        </section>

        <section id="start" className="band start">
          <div className="start-main reveal">
            <h2>Hand your agent the prompt</h2>
            <p>Copy it, paste it into your coding agent and tell it which machine or SSH host it may use.</p>
            <a className="button" href="#setup-prompt">Go to the setup prompt</a>
          </div>
          <div className="start-manual reveal">
            <h3>Prefer to do it yourself?</h3>
            <p>Build from source with Cargo, then run <code>vanityctl install --start --codex-skill</code>. It sets up the daemon and private API, and leaves your workloads alone.</p>
            <p><code>vanityctl install --prompt</code> prints the same setup prompt.</p>
            <a className="text-link" href={`${docs}/guide.md#manual-installation`}>Manual installation guide</a>
          </div>
        </section>
      </main>

      <footer className="site-footer">
        <p className="footer-mark" aria-hidden="true">vanityctl<span className="cursor" /></p>
        <div className="footer-meta">
          <p>Agent-managed self-hosted hardware. MIT licensed, written in Rust, macOS first.</p>
          <nav aria-label="Footer">
            {footerLinks.map(([label, href]) => <a href={href} key={label}>{label}</a>)}
          </nav>
        </div>
      </footer>
    </>
  );
}
