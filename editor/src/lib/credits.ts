/**
 * The projects jEngine is built on, for the About box.
 *
 * Each names the crate or npm package it is, so the dialog shows the version
 * that actually ships (from `generated/third-party.json`, written by
 * `tools/release/third_party_notices.mjs`) - and
 * `editor/scripts/help.test.ts` fails a credit whose crate or package is no
 * longer in that list, so a credit cannot outlive the dependency.
 */
import thirdParty from "./generated/third-party.json";

export interface Credit {
  name: string;
  /** What jEngine uses it for. */
  role: string;
  url: string;
  /** Where its version and licence come from. */
  crate?: string;
  npm?: string;
  /** A bundled asset, by its name in third-party.json's `assets`. */
  asset?: string;
}

export const CREDITS: Credit[] = [
  { name: "wgpu", role: "rendering on Vulkan, Metal, DX12, WebGPU and WebGL", url: "https://wgpu.rs", crate: "wgpu" },
  { name: "naga", role: "shader translation", url: "https://github.com/gfx-rs/wgpu/tree/trunk/naga", crate: "naga" },
  { name: "Rapier", role: "2D physics", url: "https://rapier.rs", crate: "rapier2d" },
  { name: "Parry", role: "collision geometry", url: "https://parry.rs", crate: "parry2d" },
  { name: "hecs", role: "the entity-component store", url: "https://github.com/Ralith/hecs", crate: "hecs" },
  { name: "mlua", role: "native Lua scripting", url: "https://github.com/mlua-rs/mlua", crate: "mlua" },
  { name: "Luau", role: "the native Lua VM", url: "https://luau.org", crate: "luau0-src" },
  {
    name: "Fengari and Lua",
    role: "the Lua 5.3 VM of the editor and web builds",
    url: "https://fengari.io",
    npm: "fengari",
  },
  { name: "QuickJS (rquickjs)", role: "JavaScript scripts on desktop", url: "https://github.com/DelSkayn/rquickjs", crate: "rquickjs" },
  { name: "Kira", role: "audio mixing and playback", url: "https://github.com/tesselode/kira", crate: "kira" },
  { name: "Symphonia", role: "audio decoding", url: "https://github.com/pdeljanov/Symphonia", crate: "symphonia" },
  { name: "winit", role: "windows and input (a patched copy)", url: "https://github.com/rust-windowing/winit", asset: "winit (patched copy)" },
  { name: "gilrs", role: "gamepads", url: "https://gitlab.com/gilrs-project/gilrs", crate: "gilrs" },
  { name: "AccessKit", role: "screen readers", url: "https://accesskit.dev", crate: "accesskit" },
  { name: "rustybuzz", role: "text shaping", url: "https://github.com/harfbuzz/rustybuzz", crate: "rustybuzz" },
  { name: "Lyon", role: "path tessellation", url: "https://github.com/nical/lyon", crate: "lyon" },
  { name: "image", role: "image decoding", url: "https://github.com/image-rs/image", crate: "image" },
  { name: "Serde", role: "every file format", url: "https://serde.rs", crate: "serde" },
  { name: "Tokio and axum", role: "the game server", url: "https://tokio.rs", crate: "axum" },
  { name: "Tauri", role: "the editor's shell", url: "https://tauri.app", crate: "tauri" },
  { name: "Svelte", role: "the editor's interface", url: "https://svelte.dev", npm: "svelte" },
  { name: "CodeMirror", role: "the script editor", url: "https://codemirror.net", npm: "codemirror" },
  { name: "Noto Sans", role: "the engine's built-in font", url: "https://notofonts.github.io", asset: "Noto Sans" },
  { name: "IBM Plex", role: "the editor's type", url: "https://www.ibm.com/plex/", npm: "@fontsource/ibm-plex-sans" },
  { name: "Dela Gothic One", role: "the wordmark's display face", url: "https://fonts.google.com/specimen/Dela+Gothic+One", npm: "@fontsource/dela-gothic-one" },
];

interface Listed {
  name: string;
  version?: string;
  license: string;
}

/** The version and licence a credit ships with, from third-party.json. */
export function creditDetails(credit: Credit): { version: string; license: string } | null {
  let found: Listed | undefined;
  if (credit.crate) found = lastByName(thirdParty.rust, credit.crate);
  else if (credit.npm) found = lastByName(thirdParty.npm, credit.npm);
  else if (credit.asset) found = thirdParty.assets.find((a) => a.name === credit.asset);
  return found ? { version: found.version ?? "", license: found.license } : null;
}

/** The newest version when several ship (sorted by version in the file). */
function lastByName(list: Listed[], name: string): Listed | undefined {
  const all = list.filter((p) => p.name === name);
  return all[all.length - 1];
}
