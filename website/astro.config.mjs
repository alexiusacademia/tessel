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
						{ label: 'The REPL', slug: 'start/repl' },
						{ label: 'Editor support', slug: 'start/editors' },
					],
				},
				{
					label: 'Learn to program',
					items: [
						{ label: 'Start here', slug: 'learn' },
						{
							label: 'Foundations',
							collapsed: true,
							items: [
								{ label: '1. Your first program', slug: 'learn/01-first-program' },
								{ label: '2. Values and variables', slug: 'learn/02-values-variables' },
								{ label: '3. Arithmetic', slug: 'learn/03-arithmetic' },
								{ label: '4. Making decisions', slug: 'learn/04-decisions' },
								{ label: '5. Loops', slug: 'learn/05-loops' },
							],
						},
						{
							label: 'Organizing code and data',
							collapsed: true,
							items: [
								{ label: '6. Functions', slug: 'learn/06-functions' },
								{ label: '7. Lists', slug: 'learn/07-lists' },
								{ label: '8. Working with text', slug: 'learn/08-text' },
								{ label: '9. Dictionaries', slug: 'learn/09-dictionaries' },
								{ label: '10. Missing values and input', slug: 'learn/10-optionals-input' },
							],
						},
						{
							label: 'Modeling your world',
							collapsed: true,
							items: [
								{ label: '11. Structs', slug: 'learn/11-structs' },
								{ label: '12. Enums and match', slug: 'learn/12-enums' },
								{ label: '13. Functions as values', slug: 'learn/13-closures' },
								{ label: '14. Interfaces', slug: 'learn/14-interfaces' },
								{ label: '15. Generics', slug: 'learn/15-generics' },
							],
						},
						{
							label: 'Building real programs',
							collapsed: true,
							items: [
								{ label: '16. Files and saving data', slug: 'learn/16-files-data' },
								{ label: '17. Algorithms', slug: 'learn/17-algorithms' },
								{ label: '18. Project: a to-do list in the terminal', slug: 'learn/18-project-cli' },
								{ label: '19. Your first app with a window', slug: 'learn/19-first-app' },
								{ label: '20. Project: a complete app', slug: 'learn/20-project-app' },
								{ label: '21. Where to go next', slug: 'learn/21-next-steps' },
							],
						},
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
						{ label: 'Generics', slug: 'language/generics' },
						{ label: 'Control flow', slug: 'language/control-flow' },
						{ label: 'Optionals', slug: 'language/optionals' },
						{ label: 'Errors', slug: 'language/errors' },
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
						{ label: 'Drawing and the mouse', slug: 'ui/drawing' },
						{ label: 'Code editor', slug: 'ui/code-editor' },
						{ label: 'Files, commands and the network', slug: 'ui/async' },
						{ label: 'Keyboard shortcuts', slug: 'ui/shortcuts' },
						{ label: 'Menus, windows and documents', slug: 'ui/menus-windows' },
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
						{ label: "What's new", slug: 'about/whats-new' },
						{ label: 'Design decisions', slug: 'about/design' },
						{ label: 'Roadmap', slug: 'about/roadmap' },
						{ label: 'Building from source', slug: 'about/source' },
					],
				},
			],
		}),
	],
});
