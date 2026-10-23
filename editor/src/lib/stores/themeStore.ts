import { writable } from "svelte/store";

// The editor's look: one of the website's palettes, dark or light. The values
// live in `src/theme.css`; this only says which block applies, by putting
// `data-palette` / `data-theme` on <html>. Kept in localStorage under the same
// keys the website uses, and applied on import - `main.ts` imports this before
// mounting, so the first paint is already in the right palette.

export type Palette = "dusk" | "sabers" | "genom";
export type ThemeMode = "dark" | "light";

export const PALETTES: { id: Palette; label: string }[] = [
  { id: "dusk", label: "MegaTokyo Dusk" },
  { id: "sabers", label: "Knight Sabers" },
  { id: "genom", label: "GENOM Tower" },
];

const PALETTE_KEY = "jx-palette";
const MODE_KEY = "jx-theme";

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Private storage: the choice lasts for this session only.
  }
}

function initialPalette(): Palette {
  const v = read(PALETTE_KEY);
  return PALETTES.some((p) => p.id === v) ? (v as Palette) : "dusk";
}

function initialMode(): ThemeMode {
  return read(MODE_KEY) === "light" ? "light" : "dark";
}

export const palette = writable<Palette>(initialPalette());
export const themeMode = writable<ThemeMode>(initialMode());

// index.html paints the background inline before any CSS loads; from here on
// theme.css owns it.
document.documentElement.style.background = "";

palette.subscribe((p) => {
  document.documentElement.dataset.palette = p;
  write(PALETTE_KEY, p);
});

themeMode.subscribe((m) => {
  document.documentElement.dataset.theme = m;
  write(MODE_KEY, m);
});
