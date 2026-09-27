// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { readFileSync } from 'node:fs';
import retinaImages from './src/plugins/retina-images.mjs';

// Tessel code blocks use the same grammar as the VS Code extension.
const tessel = JSON.parse(readFileSync(new URL('../editors/vscode/syntaxes/tessel.tmLanguage.json', import.meta.url), 'utf8'));

export default defineConfig({
	site: 'https://tessel-lang.netlify.app',
	markdown: { rehypePlugins: [retinaImages] },
	integrations: [
		starlight({
			title: 'Tessel',
			description: 'A small, declarative, compiled language for building desktop apps on macOS and Windows.',
			logo: { src: './src/assets/logo.svg' },
			customCss: ['./src/styles/custom.css'],
			expressiveCode: {
				shiki: { langs: [{ ...tessel, name: 'tessel', aliases: ['tsl'] }] },
			},
			sidebar: [
				{
					label: 'Getting started',
					items: [
						{ label: 'Introduction', slug: 'start/introduction' },
						{ label: 'Installation', slug: 'start/installation' },
						{ label: 'Your first app', slug: 'start/first-app' },
						{ label: 'The Tessel IDE', slug: 'start/ide' },
						{ label: 'Command line', slug: 'start/cli' },
						{ label: 'Editor support', slug: 'start/editors' },
					],
				},
				{
					label: 'The language',
					items: [
						{ label: 'Basics', slug: 'language/basics' },
						{ label: 'Types', slug: 'language/types' },
						{ label: 'Functions', slug: 'language/functions' },
						{ label: 'Structs and enums', slug: 'language/structs-enums' },
						{ label: 'Interfaces', slug: 'language/interfaces' },
						{ label: 'Control flow', slug: 'language/control-flow' },
						{ label: 'Optionals and errors', slug: 'language/optionals' },
						{ label: 'Collections', slug: 'language/collections' },
						{ label: 'Closures', slug: 'language/closures' },
						{ label: 'Values and copying', slug: 'language/values' },
						{ label: 'Programs and files', slug: 'language/programs' },
					],
				},
				{
					label: 'Building apps',
					items: [
						{ label: 'Apps and views', slug: 'ui/apps-views' },
						{ label: 'State and bindings', slug: 'ui/state' },
						{ label: 'Layout', slug: 'ui/layout' },
						{ label: 'Alignment and sizing', slug: 'ui/alignment' },
						{ label: 'Text and styling', slug: 'ui/styling' },
						{ label: 'Controls', slug: 'ui/controls' },
						{ label: 'Lists and identity', slug: 'ui/lists' },
						{ label: 'Tabs', slug: 'ui/tabs' },
						{ label: 'Icons, SVG and images', slug: 'ui/icons' },
						{ label: 'Code editor', slug: 'ui/code-editor' },
						{ label: 'Files, commands and the network', slug: 'ui/async' },
						{ label: 'Keyboard shortcuts', slug: 'ui/shortcuts' },
						{ label: 'Testing apps', slug: 'ui/testing' },
					],
				},
				{
					label: 'Guides',
					items: [
						{ label: 'Saving data as JSON', slug: 'guides/json' },
					],
				},
				{
					label: 'Reference',
					items: [
						{ label: 'Views', slug: 'reference/views' },
						{ label: 'Modifiers', slug: 'reference/modifiers' },
						{ label: 'Standard library', slug: 'reference/stdlib' },
						{ label: 'Icons', slug: 'reference/icons' },
						{ label: 'Colors', slug: 'reference/colors' },
						{ label: 'Grammar', slug: 'reference/grammar' },
					],
				},
				{
					label: 'About',
					items: [
						{ label: 'Design decisions', slug: 'about/design' },
						{ label: 'Roadmap', slug: 'about/roadmap' },
						{ label: 'Building from source', slug: 'about/source' },
					],
				},
			],
		}),
	],
});
