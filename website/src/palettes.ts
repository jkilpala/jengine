// The site's palettes (colours in src/styles/palettes.css). Dusk is the
// default and is the absence of a `data-palette` attribute.

export const PALETTES = [
	{ id: 'dusk', label: 'Dusk' },
	{ id: 'sabers', label: 'Sabers' },
	{ id: 'genom', label: 'GENOM' },
] as const;

export type PaletteId = (typeof PALETTES)[number]['id'];

const KEY = 'jx-palette';

function isPalette(v: unknown): v is PaletteId {
	return PALETTES.some((p) => p.id === v);
}

/** The palette on the page, as the boot script (src/palette-boot.mjs) applied it. */
export function currentPalette(): PaletteId {
	const shown = document.documentElement.dataset.palette;
	return isPalette(shown) ? shown : 'dusk';
}

/** Switch palette, remember it, and keep every picker on the page in step. */
export function applyPalette(id: string): void {
	const palette = isPalette(id) ? id : 'dusk';
	if (palette === 'dusk') delete document.documentElement.dataset.palette;
	else document.documentElement.dataset.palette = palette;
	try {
		if (palette === 'dusk') localStorage.removeItem(KEY);
		else localStorage.setItem(KEY, palette);
	} catch {
		// Not remembered, but still applied for this page.
	}
	document.querySelectorAll<HTMLSelectElement>('[data-palette-select] select, jx-palette-select select').forEach((s) => {
		s.value = palette;
	});
}
