# Tessel website

The Tessel website and documentation, built with
[Astro Starlight](https://starlight.astro.build).

```sh
npm install
npm run dev       # http://localhost:4321
npm run build     # static site in dist/
```

Pages are Markdown files in `src/content/docs/`; the sidebar is in
`astro.config.mjs`. Tessel code blocks (```tessel) are highlighted with the
grammar from `editors/vscode/syntaxes/`. The icon gallery is
generated from `crates/tessel-runtime/icons/` at build time.

## Deploying to Netlify

The site is live at https://tessel-lang.netlify.app (Netlify project
`tessel-lang`, team syncster.dev). `netlify.toml` in this folder has the build
settings; the base directory is `website`.

It's deployed by `.github/workflows/website.yml` whenever a push to `master`
changes this folder (or the syntax grammar or the icons, which it's built
from). The workflow runs on the self-hosted runner and uses the Netlify
login of that machine (`npx netlify login`); there's no token in GitHub.

To deploy by hand from this folder (it's linked with `netlify link`):

```sh
npx netlify deploy --build --prod
```
