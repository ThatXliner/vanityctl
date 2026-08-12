const github = "https://github.com/ThatXliner/vanityctl";

const workloads = [
  { name: "billion", type: "docker", state: "running", detail: "main@a81d3f" },
  { name: "minecraft", type: "docker", state: "running", detail: ":25565" },
  { name: "local-llm", type: "process", state: "running", detail: "pid 4921" },
  { name: "scraper", type: "job", state: "idle", detail: "last run ✓" },
  { name: "cloudflare-ddns", type: "job", state: "idle", detail: "synced ✓" },
];

const serviceTypes = [
  {
    index: "01",
    title: "Containers",
    body: "Run a single image, build a Dockerfile, or keep an existing Compose stack intact.",
    code: "type: docker",
  },
  {
    index: "02",
    title: "Native processes",
    body: "Treat host binaries and scripts as first-class workloads, supervised by launchd.",
    code: "type: process",
  },
  {
    index: "03",
    title: "Scheduled jobs",
    body: "Run backups, scrapers, and maintenance on a schedule with history and logs.",
    code: "type: job",
  },
  {
    index: "04",
    title: "Git deployments",
    body: "Pull an exact commit, run explicit hooks, rebuild safely, and record what happened.",
    code: "deploy: { auto: true }",
  },
];

const commands = [
  ["See the whole machine", "vanityctl status"],
  ["Inspect one service", "vanityctl describe billion --json"],
  ["Operate consistently", "vanityctl restart minecraft"],
  ["Deploy deterministically", "vanityctl deploy billion"],
  ["Run a scheduled job now", "vanityctl run scraper"],
  ["Reconcile desired state", "vanityctl apply --dry-run"],
];

export default function Home() {
  return (
    <main>
      <header className="site-header shell">
        <a className="wordmark" href="#top" aria-label="vanityctl home">
          <span aria-hidden="true">{`{`}</span> vanityctl <span aria-hidden="true">{`}`}</span>
        </a>
        <nav aria-label="Primary navigation">
          <a href="#why">Why</a>
          <a href="#model">Model</a>
          <a href="#dogfood">Dogfood</a>
          <a className="nav-github" href={github}>GitHub ↗</a>
        </nav>
      </header>

      <section className="hero shell" id="top">
        <div className="hero-copy">
          <p className="eyebrow"><span>Open source</span><span>macOS first</span><span>one machine</span></p>
          <h1>Everything this machine is responsible for.</h1>
          <p className="hero-deck">
            One declarative registry and one predictable command surface for Docker,
            Compose, native processes, scheduled jobs, Git deployments, and DNS.
          </p>
          <div className="hero-actions">
            <a className="button button-primary" href={github}>View on GitHub <span>↗</span></a>
            <a className="button button-secondary" href={`${github}#quick-start`}>Quick start <span>→</span></a>
          </div>
          <p className="install"><span>$</span> cargo install --git {github}.git</p>
        </div>

        <div className="terminal-wrap" aria-label="Example vanityctl status output">
          <div className="terminal-chrome">
            <span className="terminal-title">big-mac / status</span>
            <span className="terminal-live"><i /> live</span>
          </div>
          <div className="terminal">
            <p><b>$</b> vanityctl status</p>
            <div className="terminal-table terminal-head">
              <span>NAME</span><span>TYPE</span><span>STATE</span><span>DETAILS</span>
            </div>
            {workloads.map((item) => (
              <div className="terminal-table" key={item.name}>
                <span>{item.name}</span>
                <span className="terminal-muted">{item.type}</span>
                <span className={`state state-${item.state}`}><i />{item.state}</span>
                <span className="terminal-muted">{item.detail}</span>
              </div>
            ))}
            <div className="terminal-prompt"><b>$</b><span className="cursor" /></div>
          </div>
          <div className="terminal-caption">
            Same verbs. Correct backend. No operational archaeology.
          </div>
        </div>
      </section>

      <section className="manifesto" id="why">
        <div className="shell manifesto-grid">
          <p className="section-kicker">The question</p>
          <div>
            <p className="quote">“What should be running on this machine?”</p>
            <p className="manifesto-copy">
              Most tools begin with a technology: containers, processes, applications,
              or clusters. vanityctl begins with the computer. Every workload becomes a
              peer in one version-controlled model—without pretending everything is Docker.
            </p>
          </div>
        </div>
      </section>

      <section className="service-model shell" id="model">
        <div className="section-heading">
          <div><p className="section-kicker">One service model</p><h2>Your server is not one thing.</h2></div>
          <p>Five services or one hundred. The operational vocabulary stays small.</p>
        </div>
        <div className="service-grid">
          {serviceTypes.map((service) => (
            <article className="service-card" key={service.index}>
              <div className="service-card-top"><span>{service.index}</span><code>{service.code}</code></div>
              <h3>{service.title}</h3>
              <p>{service.body}</p>
            </article>
          ))}
        </div>
      </section>

      <section className="architecture-section">
        <div className="shell">
          <div className="section-heading architecture-heading">
            <div><p className="section-kicker">The control plane</p><h2>Boring by design.</h2></div>
            <p>vanityctl uses the reliable tools already on your machine. It does not replace them.</p>
          </div>
          <div className="architecture" role="img" aria-label="YAML desired state flows through hostd to Docker, launchd, Git, and DNS, with CLI and web clients">
            <div className="architecture-node architecture-source"><small>DESIRED STATE</small><strong>YAML registry</strong><span>version controlled</span></div>
            <div className="architecture-line"><span>↓</span></div>
            <div className="architecture-node architecture-hostd"><small>CONTROL PLANE</small><strong>hostd</strong><span>localhost API · source of truth</span></div>
            <div className="architecture-line architecture-split"><span>↙</span><span>↓</span><span>↓</span><span>↘</span></div>
            <div className="architecture-targets">
              <div><strong>Docker</strong><span>containers + Compose</span></div>
              <div><strong>launchd</strong><span>processes + jobs</span></div>
              <div><strong>Git</strong><span>deploys + polling</span></div>
              <div><strong>DNS</strong><span>Cloudflare records</span></div>
            </div>
            <div className="architecture-clients"><span>vanityctl CLI</span><i>↔</i><span>local dashboard</span></div>
          </div>
        </div>
      </section>

      <section className="commands-section shell">
        <div className="commands-copy">
          <p className="section-kicker">Predictable operations</p>
          <h2>A small command surface for humans and agents.</h2>
          <p>
            Stop asking every repository how it wants to be deployed. The registry holds
            that knowledge once; the CLI and JSON API expose it everywhere.
          </p>
          <a href={`${github}#ai-agent-integration`}>AI-agent integration →</a>
        </div>
        <div className="command-list">
          {commands.map(([label, command]) => (
            <div className="command-row" key={command}>
              <span>{label}</span><code><b>$</b> {command}</code>
            </div>
          ))}
        </div>
      </section>

      <section className="dogfood" id="dogfood">
        <div className="shell dogfood-grid">
          <div>
            <p className="section-kicker section-kicker-light">Dogfooded on a real Mac Studio</p>
            <h2>11 existing stacks.<br />Zero containers replaced.</h2>
            <p>
              vanityctl was installed on <code>big-mac</code>, adopted eleven live Compose
              projects, reconciled them safely, and surfaced an unhealthy relay the old
              machine-wide view could not show.
            </p>
          </div>
          <div className="proof-grid">
            <div><strong>11</strong><span>Compose projects adopted</span></div>
            <div><strong>0</strong><span>container IDs changed</span></div>
            <div><strong>0 / 11</strong><span>changed / unchanged on second apply</span></div>
            <div><strong>1</strong><span>unhealthy workload surfaced</span></div>
          </div>
        </div>
      </section>

      <section className="not-kubernetes shell">
        <div className="not-kubernetes-title"><p className="section-kicker">The niche</p><h2>Not the smallest Kubernetes.</h2></div>
        <div className="not-kubernetes-copy">
          <p>Not a container dashboard. Not a replacement init system. Not another PaaS.</p>
          <p>
            vanityctl is a <strong>single-node declarative control plane</strong> for
            self-hosters and developers. Docker runs containers. launchd supervises
            processes. Git stores source. vanityctl makes the whole machine legible.
          </p>
          <a href={`${github}/blob/main/docs/comparison.md`}>Compare with existing tools →</a>
        </div>
      </section>

      <section className="final-cta">
        <div className="shell final-cta-inner">
          <div><p className="section-kicker section-kicker-light">Build the boring layer</p><h2>Make your machine explain itself.</h2></div>
          <div><a className="button button-light" href={github}>Explore vanityctl on GitHub <span>↗</span></a><p>MIT licensed · Rust · macOS first</p></div>
        </div>
      </section>

      <footer className="site-footer shell">
        <a className="wordmark" href="#top"><span>{`{`}</span> vanityctl <span>{`}`}</span></a>
        <p>One control plane for this computer.</p>
        <div><a href={github}>GitHub</a><a href={`${github}#quick-start`}>Docs</a><a href={`${github}/blob/main/LICENSE`}>MIT License</a></div>
      </footer>
    </main>
  );
}
