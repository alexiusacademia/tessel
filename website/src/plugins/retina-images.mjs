// Screenshots are taken at 2x (Retina). Show them at half their pixel size,
// which is the app's real size in points, so they're sharp and not huge.
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';

function pngSize(path) {
	const b = readFileSync(path);
	// PNG: width and height are big-endian u32s at bytes 16 and 20.
	return { width: b.readUInt32BE(16), height: b.readUInt32BE(20) };
}

function visit(node, fn) {
	fn(node);
	for (const c of node.children ?? []) visit(c, fn);
}

export default function retinaImages() {
	return (tree, file) => {
		visit(tree, (node) => {
			if (node.type !== 'element' || node.tagName !== 'img') return;
			const src = node.properties?.src;
			if (typeof src !== 'string' || !src.includes('/screens/') || !src.endsWith('.png')) return;
			if (node.properties.style) return;
			try {
				const { width } = pngSize(resolve(dirname(file.path), src));
				// Sized with CSS, so the full-resolution image is still used.
				node.properties.style = `width: ${Math.round(width / 2)}px; max-width: 100%; height: auto;`;
			} catch {
				// Leave the image as it is if it can't be read.
			}
		});
	};
}
