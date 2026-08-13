const github = "https://github.com/ThatXliner/vanityctl";

const services = [
  ["billion", "docker", "running", "main@a81d3f"],
  ["minecraft", "docker", "running", ":25565"],
  ["local-llm", "process", "running", "pid 4921"],
  ["scraper", "job", "idle", "last run ✓"],
  ["cloudflare-ddns", "job", "idle", "synced ✓"],
];

function Prompt({ children }: { children: React.ReactNode }) {
  return <div className="prompt"><span className="prompt-host">big-mac</span><span className="prompt-path">~</span><span className="prompt-mark">❯</span><span>{children}</span></div>;
}

export default function Home() {
  return (
    <main id="top">
      <div className="terminal-window">
        <header className="titlebar">
          <div className="traffic" aria-hidden="true"><i /><i /><i /></div>
          <span>big-mac — vanityctl — 120×40</span>
          <a href={github} aria-label="Open vanityctl on GitHub">github ↗</a>
        </header>

        <nav aria-label="Page sections">
          <a href="#why">01 why</a><a href="#model">02 model</a>
          <a href="#dogfood">03 dogfood</a><a href="#install">04 install</a>
        </nav>

        <div className="session">
          <section className="hero">
            <Prompt>vanityctl about</Prompt>
            <div className="hero-output output-block">
              <p className="comment"># one control plane for this computer</p>
              <h1>Everything this machine<br />is responsible for.</h1>
              <p className="lede">One declarative registry and one predictable command surface for Docker, Compose, native processes, scheduled jobs, Git deployments, and DNS.</p>
              <div className="hero-links"><a href={github}>[ view source ↗ ]</a><a href={`${github}/blob/main/docs/README.md`}>[ read the docs ]</a></div>
            </div>
          </section>

          <section className="status-section" aria-label="Example service status">
            <Prompt>vanityctl status <span className="flag">--all</span></Prompt>
            <div className="status-table output-block">
              <div className="status-row status-head"><span>NAME</span><span>TYPE</span><span>STATE</span><span>DETAILS</span></div>
              {services.map(([name, type, state, detail]) => (
                <div className="status-row" key={name}>
                  <span>{name}</span><span className="muted">{type}</span>
                  <span className={`status-${state}`}><i />{state}</span><span className="muted">{detail}</span>
                </div>
              ))}
              <p className="success">✓ 5 services · 0 operational mysteries</p>
            </div>
          </section>

          <section id="why">
            <Prompt>vanityctl principles</Prompt>
            <div className="principles output-block">
              <article><span className="index">01</span><h2>organized</h2><p>Every service and its operational metadata lives in one inspectable, version-controlled registry. No more deployment instructions scattered across repos, plists, scripts, and memory.</p></article>
              <article><span className="index">02</span><h2>unified</h2><p>Every workload uses the same small vocabulary. Humans and agents run <code>status</code>, <code>logs</code>, <code>restart</code>, and <code>deploy</code>; vanityctl chooses the backend.</p></article>
              <article><span className="index">03</span><h2>simple</h2><p>Compose stays Compose. launchd stays launchd. Configuration is ordinary YAML, persistent data stays yours, and uninstalling does not require escaping a platform.</p></article>
            </div>
          </section>

          <section id="model">
            <Prompt>cat ~/.vanityctl/config.yaml</Prompt>
            <div className="code-output output-block" aria-label="Example vanityctl YAML configuration">
              <pre><code><span className="key">version:</span> <span className="value">1</span>{`\n\n`}<span className="key">services:</span>{`\n`}  <span className="service">minecraft:</span>{`\n`}    <span className="key">type:</span> docker{`\n`}    <span className="key">image:</span> itzg/minecraft-server{`\n`}    <span className="key">ports:</span> [<span className="string">&quot;25565:25565&quot;</span>]{`\n`}    <span className="key">restart:</span> always{`\n\n`}  <span className="service">local-llm:</span>{`\n`}    <span className="key">type:</span> process{`\n`}    <span className="key">command:</span> ./serve.sh{`\n`}    <span className="key">restart:</span> always{`\n\n`}  <span className="service">scraper:</span>{`\n`}    <span className="key">type:</span> job{`\n`}    <span className="key">command:</span> ./scrape.sh{`\n`}    <span className="key">schedule:</span> <span className="string">&quot;0 4 * * *&quot;</span></code></pre>
              <div className="annotation"><span>one file</span><span>three runtimes</span><span>same lifecycle</span></div>
            </div>
          </section>

          <section>
            <Prompt>vanityctl architecture <span className="flag">--tree</span></Prompt>
            <div className="tree output-block" role="img" aria-label="vanityctl architecture tree">
              <p><b>~/.vanityctl/</b></p>
              <p>├── <b>config.yaml</b> <em># desired state</em></p>
              <p>└── <span className="accent">hostd</span> <em># localhost control plane</em></p>
              <p>    ├── docker <em>containers + Compose</em></p>
              <p>    ├── launchd <em>processes + jobs</em></p>
              <p>    ├── git <em>deploys + polling</em></p>
              <p>    └── dns <em>Cloudflare records</em></p>
              <p className="tree-gap">        ▲</p>
              <p>        ├── vanityctl <em>CLI + stable JSON</em></p>
              <p>        └── dashboard <em>same API, no orchestration logic</em></p>
            </div>
          </section>

          <section className="commands">
            <Prompt>vanityctl help <span className="flag">--short</span></Prompt>
            <div className="help-output output-block">
              <p><span>status</span><code>vanityctl status [service]</code><em>see the whole machine</em></p>
              <p><span>inspect</span><code>vanityctl describe billion --json</code><em>read operational intent</em></p>
              <p><span>operate</span><code>vanityctl restart minecraft</code><em>use the correct backend</em></p>
              <p><span>deploy</span><code>vanityctl deploy billion</code><em>fetch, build, replace safely</em></p>
              <p><span>run</span><code>vanityctl run scraper</code><em>execute a scheduled job now</em></p>
              <p><span>reconcile</span><code>vanityctl apply --dry-run</code><em>preview desired-state changes</em></p>
            </div>
          </section>

          <section id="dogfood">
            <Prompt>vanityctl apply <span className="flag">--host big-mac</span></Prompt>
            <div className="dogfood-output output-block">
              <p><span className="log-time">16:04:12</span> inspecting existing Compose projects...</p>
              <p><span className="log-time">16:04:13</span> found <b>11</b> projects and <b>73</b> containers</p>
              <p><span className="log-time">16:04:14</span> reconciling without replacement...</p>
              <p><span className="log-time">16:04:15</span> <span className="success">✓ apply complete</span></p>
              <div className="proof-line"><span><b>11</b> adopted</span><span><b>0</b> container IDs changed</span><span><b>0 / 11</b> drift on second apply</span><span><b>1</b> unhealthy service surfaced</span></div>
              <p className="comment"># dogfooded on a real Mac Studio, not a slide deck</p>
            </div>
          </section>

          <section className="positioning">
            <Prompt>vanityctl explain <span className="flag">--not-kubernetes</span></Prompt>
            <div className="output-block">
              <p className="error">not a container dashboard. not a replacement init system. not another PaaS.</p>
              <p>Docker runs containers. Compose defines applications. launchd supervises host processes. Git stores source. <b>vanityctl makes the whole machine legible.</b></p>
              <a href={`${github}/blob/main/docs/comparison.md`}>→ compare with existing tools</a>
            </div>
          </section>

          <section id="install" className="install-section">
            <Prompt>cargo install <span className="flag">--git</span> https://github.com/ThatXliner/vanityctl</Prompt>
            <div className="install-output output-block">
              <p>Installing vanityctl v0.1.0...</p><p>Installing hostd v0.1.0...</p><p className="success">✓ ready</p>
              <h2>Make your machine<br />explain itself.</h2>
              <div className="cta"><a href={github}>view source on GitHub ↗</a><a href={`${github}/blob/main/docs/guide.md`}>read the complete guide →</a></div>
            </div>
          </section>

          <footer>
            <Prompt><span className="cursor" aria-hidden="true" /></Prompt>
            <p>MIT licensed · Rust · macOS first · <a href="#top">back to top ↑</a></p>
          </footer>
        </div>
      </div>
    </main>
  );
}
