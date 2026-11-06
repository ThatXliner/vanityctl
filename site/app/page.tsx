import { CopyPrompt } from "../components/copy-prompt";
import { ServiceDemo } from "../components/service-demo";

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
          <p className="tagline"><span>Agent-managed</span> <span>self-hosted hardware.</span></p>
          <p className="intro">One setup prompt for Codex, Claude Code or your agent.</p>
          <CopyPrompt />
        </section>

        <section className="film" aria-label="vanityctl in 22 seconds">
          <video src="brag.mp4" poster="brag.jpg" controls playsInline preload="none">
            <track kind="captions" src="brag.vtt" srcLang="en" label="English" />
          </video>
        </section>

        <section className="demo" aria-labelledby="demo-heading">
          <h2 id="demo-heading">What your agent sees</h2>
          <ServiceDemo />
          <p className="links">
            <a href={`${docs}/onboarding.md`}>Onboarding guide</a>
            <a href={`${docs}/agent-workflows.md`}>Agent workflows</a>
            <a href={`${docs}/README.md`}>All docs</a>
          </p>
        </section>
      </main>

      <footer className="site-footer">
        <p>macOS first · MIT licensed</p>
        <a href={github}>github.com/ThatXliner/vanityctl</a>
      </footer>
    </div>
  );
}
