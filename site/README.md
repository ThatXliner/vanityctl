# vanityctl landing page

The public landing page for [vanityctl](https://github.com/ThatXliner/vanityctl),
built with React, TypeScript, vinext, and Vite.

## Local development

Requires Node.js 22 or newer.

```bash
npm install
npm run dev
```

The development server uses `/` locally. The GitHub Actions build sets the
production asset base to `/vanityctl/`.

## Verification

```bash
npm test
npm run lint
GITHUB_ACTIONS=true npm run build:pages
```

`build:pages` renders the landing page to static HTML in `out/`. The workflow at
`.github/workflows/pages.yml` uploads that directory to GitHub Pages whenever
landing-page files change on `main`.
