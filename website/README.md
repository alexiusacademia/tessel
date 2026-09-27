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

`netlify.toml` at the repository root has the settings: base directory
`website`, build command `npm run build`, publish directory `dist`. Connect the
repository in Netlify and deploy; no other configuration is needed.
