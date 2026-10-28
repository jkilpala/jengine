#!/usr/bin/env node
// Third-party notices for a jEngine release.
//
// Usage (from anywhere; paths are taken from this file's place in the repo):
//
//   node tools/release/third_party_notices.mjs           write both files
//   node tools/release/third_party_notices.mjs --check   write nothing; exit 1 if either is stale
//
// It writes:
//   THIRD-PARTY-NOTICES.md                       every crate, npm package and bundled asset
//                                                that ships, with each distinct licence text once
//   editor/src/lib/generated/third-party.json    the same list, compact, for the editor's
//                                                licences dialog (no licence texts)
//
// Inputs: `cargo metadata` (offline, against the existing Cargo.lock - it never builds) for the
// shipped binaries' crates and their normal + build dependencies on every platform, and
// `editor/node_modules` for the editor's production npm dependencies. The bundled assets are
// listed by hand below (ASSETS). The output is deterministic: sorted, LF, no dates, no absolute
// paths - rerun it after changing Cargo.lock or editor/package-lock.json.
//
// Node 22, no dependencies beyond node: built-ins.

import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const MD_PATH = join(ROOT, "THIRD-PARTY-NOTICES.md");
const JSON_PATH = join(ROOT, "editor", "src", "lib", "generated", "third-party.json");
const SCRIPT_REL = "tools/release/third_party_notices.mjs";

// The packages whose binaries ship.
const SHIPPED_CRATES = [
  "jengine-builder",
  "jengine-editor",
  "jengine-editor-viewport",
  "jengine-runtime",
  "jengine-server",
  "jengine-web",
];

const LICENCE_FILE = /^(licen[cs]e|copying|notice)/i;

// ---------------------------------------------------------------- helpers

const cmp = (a, b) => (a < b ? -1 : a > b ? 1 : 0);
const byNameVersion = (a, b) => cmp(a.name, b.name) || cmp(a.version, b.version);
const toPosix = (p) => p.split(sep).join("/");

/** A licence text as compared and printed: LF, no trailing whitespace, no blank edges. */
function normaliseText(text) {
  return text
    .replace(/^﻿/, "")
    .replace(/\r\n?/g, "\n")
    .split("\n")
    .map((l) => l.replace(/\s+$/, ""))
    .join("\n")
    .replace(/^\n+/, "")
    .replace(/\n+$/, "");
}

const sha = (s) => createHash("sha256").update(s).digest("hex");

function normaliseRepo(url) {
  if (!url || typeof url !== "string") return "";
  let u = url.trim();
  const short = /^(github|gitlab|bitbucket):(.+)$/.exec(u);
  if (short) {
    const host = { github: "github.com", gitlab: "gitlab.com", bitbucket: "bitbucket.org" }[short[1]];
    u = `https://${host}/${short[2]}`;
  } else if (/^[\w.-]+\/[\w.-]+$/.test(u)) {
    u = `https://github.com/${u}`;
  }
  u = u.replace(/^git\+/, "");
  u = u.replace(/^git@([^:]+):/, "https://$1/");
  u = u.replace(/^ssh:\/\/git@/, "https://");
  u = u.replace(/^git:\/\//, "https://");
  u = u.replace(/^http:\/\//, "https://");
  u = u.replace(/#.*$/, "");
  u = u.replace(/\.git$/, "");
  u = u.replace(/\/+$/, "");
  return u;
}

/** Licence files at the top of a package directory, plus any extra named ones. */
function licenceFiles(dir, extra = []) {
  const found = new Map();
  let names = [];
  try {
    names = readdirSync(dir);
  } catch {
    return [];
  }
  for (const name of names) {
    if (!LICENCE_FILE.test(name)) continue;
    const p = join(dir, name);
    try {
      if (!statSync(p).isFile()) continue;
    } catch {
      continue;
    }
    found.set(name, p);
  }
  for (const rel of extra) {
    if (!rel) continue;
    const p = resolve(dir, rel);
    if (existsSync(p) && statSync(p).isFile()) found.set(toPosix(relative(dir, p)), p);
  }
  return [...found.entries()]
    .sort((a, b) => cmp(a[0], b[0]))
    .map(([name, p]) => ({ name, text: normaliseText(readFileSync(p, "utf8")) }))
    .filter((f) => f.text.length > 0);
}

/** A fenced block that no backtick run in `text` can close early. */
function fence(text) {
  const longest = Math.max(2, ...[...text.matchAll(/`+/g)].map((m) => m[0].length));
  const f = "`".repeat(longest + 1);
  return `${f}text\n${text}\n${f}`;
}

const cell = (s) => String(s ?? "").replace(/\|/g, "\\|").replace(/\n/g, " ");

// ---------------------------------------------------------------- Rust

function cargoMetadata() {
  const base = ["metadata", "--format-version", "1", "--locked", "--manifest-path", join(ROOT, "Cargo.toml")];
  const opts = { cwd: ROOT, encoding: "utf8", maxBuffer: 1 << 30, stdio: ["ignore", "pipe", "pipe"] };
  try {
    return JSON.parse(execFileSync("cargo", [...base, "--offline"], opts));
  } catch {
    return JSON.parse(execFileSync("cargo", base, opts));
  }
}

function collectRust() {
  const meta = cargoMetadata();
  const pkgs = new Map(meta.packages.map((p) => [p.id, p]));
  const nodes = new Map(meta.resolve.nodes.map((n) => [n.id, n]));
  const roots = [];
  for (const name of SHIPPED_CRATES) {
    const p = meta.packages.find((q) => q.name === name && meta.workspace_members.includes(q.id));
    if (!p) throw new Error(`shipped crate ${name} is not a workspace member`);
    roots.push(p.id);
  }
  const seen = new Set();
  const stack = [...roots];
  while (stack.length) {
    const id = stack.pop();
    if (seen.has(id)) continue;
    seen.add(id);
    const node = nodes.get(id);
    if (!node) continue;
    for (const dep of node.deps) {
      const kinds = dep.dep_kinds ?? [];
      if (kinds.some((k) => k.kind === null || k.kind === "build")) stack.push(dep.pkg);
    }
  }
  const crates = [];
  const local = [];
  for (const id of seen) {
    const p = pkgs.get(id);
    const vendored = !p.source && toPosix(relative(ROOT, p.manifest_path)).startsWith("vendor/");
    if (!p.source) {
      // A path crate: the workspace's own, or vendored. A vendored crate is
      // still a crate that ships under its own licence, so it is listed with
      // the rest as well; its patches are described under ASSETS.
      local.push(p);
      if (!vendored) continue;
    }
    crates.push({
      name: p.name,
      version: p.version,
      license: p.license ?? (p.license_file ? `see ${p.license_file}` : "unknown"),
      repository: normaliseRepo(p.repository),
      files: licenceFiles(dirname(p.manifest_path), p.license_file ? [p.license_file] : []),
    });
  }
  crates.sort(byNameVersion);
  const vendored = local
    .filter((p) => toPosix(relative(ROOT, p.manifest_path)).startsWith("vendor/"))
    .map((p) => ({ name: p.name, version: p.version }))
    .sort(byNameVersion);
  return { crates, vendored };
}

// ---------------------------------------------------------------- npm

function readJson(p) {
  return JSON.parse(readFileSync(p, "utf8"));
}

/** Node's lookup: `from`/node_modules/name, then each parent's, up to the editor. */
function resolvePackage(name, fromDir, topDir) {
  let dir = fromDir;
  for (;;) {
    const candidate = join(dir, "node_modules", ...name.split("/"));
    if (existsSync(join(candidate, "package.json"))) return candidate;
    if (dir === topDir) return null;
    const parent = dirname(dir);
    // Climb out of a nested node_modules/<pkg> (or node_modules/@scope/<pkg>).
    if (parent === dir || !toPosix(relative(topDir, parent)).split("/").every((s) => s !== "..")) return null;
    dir = parent;
  }
}

function npmLicense(pj) {
  if (typeof pj.license === "string") return pj.license;
  if (pj.license && typeof pj.license === "object" && pj.license.type) return pj.license.type;
  if (Array.isArray(pj.licenses)) {
    const ids = pj.licenses.map((l) => (typeof l === "string" ? l : l?.type)).filter(Boolean);
    if (ids.length) return ids.length === 1 ? ids[0] : `(${ids.join(" OR ")})`;
  }
  return "unknown";
}

function npmRepo(pj) {
  const r = pj.repository;
  if (typeof r === "string") return normaliseRepo(r);
  if (r && typeof r === "object") return normaliseRepo(r.url);
  return "";
}

// devDependencies whose code ends up in the editor's bundle anyway.
const BUNDLED_DEV_PACKAGES = ["svelte", "clsx", "esm-env"];

function collectNpm() {
  const editor = join(ROOT, "editor");
  if (!existsSync(join(editor, "node_modules"))) {
    throw new Error("editor/node_modules is missing - run `npm ci --prefix editor` first");
  }
  const top = readJson(join(editor, "package.json"));
  const out = new Map();
  const missing = [];
  const queue = Object.keys(top.dependencies ?? {})
    .sort()
    .map((name) => ({ name, from: editor, optional: false, follow: true }));
  // Svelte is a devDependency, but its compiler writes the editor against its
  // runtime, which ships in the bundle - with what that runtime imports. Its
  // other dependencies are the compiler's, and stay out.
  for (const name of BUNDLED_DEV_PACKAGES) queue.push({ name, from: editor, optional: false, follow: false });
  const visited = new Set();
  while (queue.length) {
    const { name, from, optional, follow = true } = queue.shift();
    const dir = resolvePackage(name, from, editor);
    if (!dir) {
      if (!optional) missing.push(name);
      continue;
    }
    if (visited.has(dir)) continue;
    visited.add(dir);
    const pj = readJson(join(dir, "package.json"));
    const key = `${pj.name}@${pj.version}`;
    if (!out.has(key)) {
      out.set(key, {
        name: pj.name,
        version: pj.version,
        license: npmLicense(pj),
        repository: npmRepo(pj),
        files: licenceFiles(dir),
      });
    }
    if (!follow) continue;
    for (const dep of Object.keys(pj.dependencies ?? {}).sort()) queue.push({ name: dep, from: dir, optional: false });
    for (const dep of Object.keys(pj.optionalDependencies ?? {}).sort()) queue.push({ name: dep, from: dir, optional: true });
  }
  if (missing.length) throw new Error(`npm packages not installed: ${[...new Set(missing)].sort().join(", ")}`);
  return [...out.values()].sort(byNameVersion);
}

// ---------------------------------------------------------------- bundled assets

function repoText(rel) {
  const p = join(ROOT, ...rel.split("/"));
  if (!existsSync(p)) throw new Error(`licence text ${rel} is missing`);
  return { name: rel, text: normaliseText(readFileSync(p, "utf8")) };
}

function collectAssets(rust) {
  const symphonia = rust.crates.filter((c) => c.name === "symphonia" || c.name.startsWith("symphonia-"));
  const symphoniaVersions = symphonia.map((c) => `${c.name} ${c.version}`).join(", ");
  const winit = rust.vendored.find((v) => v.name === "winit");
  const rapier = rust.vendored.find((v) => v.name === "rapier2d");
  const luau = rust.vendored.find((v) => v.name === "luau0-src");
  const assets = [
    {
      name: "Noto Sans",
      license: "OFL-1.1",
      repository: "https://github.com/notofonts/latin-greek-cyrillic",
      note: "the editor and engine's built-in font",
      detail:
        "Noto Sans Regular ships as the engine's built-in font (`crates/jengine-core/assets/fonts/`, " +
        "the full face and a subset of it baked into every build) and with the editor " +
        "(`editor/public/fonts/`).",
      files: [repoText("crates/jengine-core/assets/fonts/LICENSE-NotoSans.txt")],
    },
    {
      name: "Fengari (Lua 5.3 VM in JavaScript) and Lua",
      license: "MIT",
      repository: "https://github.com/fengari-lua/fengari",
      note: "the Lua VM of the editor and of web builds, bundled in lua-runtime.js",
      detail:
        "Fengari, a port of the Lua 5.3 virtual machine to JavaScript, is bundled into the editor and into " +
        "`crates/jengine-builder/assets/lua-runtime.js`, which every web build ships. Fengari carries " +
        "Lua's own notice, Copyright (C) 1994-2017 Lua.org, PUC-Rio, reproduced in its licence below.",
      files: [repoText("editor/node_modules/fengari/LICENSE")],
    },
    {
      name: "winit (patched copy)",
      license: "Apache-2.0",
      repository: "https://github.com/rust-windowing/winit",
      note: `vendor/winit: winit ${winit ? winit.version : ""} from crates.io with jEngine patches`.replace("  ", " "),
      detail:
        `\`vendor/winit\` is winit ${winit ? winit.version : ""} from crates.io with its examples, tests and docs ` +
        "left out and three changes, each marked \"jEngine patch\" in the source: an input hook on Android " +
        "(`src/platform/android.rs`, `src/platform_impl/android/mod.rs`) so game controllers are not turned " +
        "into touches; the UIScene lifecycle on iOS (`src/platform_impl/ios/scene.rs`, new, and " +
        "`app_state.rs`, `mod.rs`, `Cargo.toml`); and on iOS a redraw request that survives `drawRect:` " +
        "(`src/platform_impl/ios/window.rs`). Every other platform is untouched. " +
        "`vendor/winit/JENGINE-PATCH.md` describes the changes in full.",
      files: [repoText("vendor/winit/LICENSE")],
    },
    {
      name: "rapier2d (patched copy)",
      license: "Apache-2.0",
      repository: "https://github.com/dimforge/rapier",
      note: `vendor/rapier2d: rapier2d ${rapier ? rapier.version : ""} from crates.io with jEngine patches`.replace("  ", " "),
      detail:
        `\`vendor/rapier2d\` is rapier2d ${rapier ? rapier.version : ""} from crates.io with one kind of change, ` +
        "each marked \"jEngine patch\" in the source: the maths it calls on `f32` directly goes through `libm`, " +
        "so a physics step is the same bits on every platform. `vendor/rapier2d/JENGINE-PATCH.md` lists every " +
        "changed file.",
      files: [repoText("vendor/rapier2d/LICENSE")],
    },
    {
      name: "Luau (luau0-src, patched copy)",
      license: "MIT",
      repository: "https://github.com/khvzak/luau-src-rs",
      note: `vendor/luau0-src: luau0-src ${luau ? luau.version : ""} from crates.io with jEngine patches`.replace("  ", " "),
      detail:
        `\`vendor/luau0-src\` is luau0-src ${luau ? luau.version : ""} from crates.io, the Luau sources, with ` +
        "changes so a script's numbers are the same on every platform: its maths calls go through a table of " +
        "functions the engine fills, `^` has one rule, and it is built without fused multiply-add. " +
        "`vendor/luau0-src/JENGINE-PATCH.md` describes the changes in full.",
      files: [repoText("vendor/luau0-src/LICENSE")],
    },
    {
      name: "Symphonia",
      license: "MPL-2.0",
      repository: "https://github.com/pdeljanov/Symphonia",
      note: `audio decoding; used unmodified from crates.io (${symphoniaVersions})`,
      detail:
        `Symphonia's crates (${symphoniaVersions}) are used unmodified, as published on crates.io. ` +
        "Their source code is available at https://github.com/pdeljanov/Symphonia and from crates.io " +
        "(https://crates.io/crates/symphonia). The MPL-2.0 text is among the Rust licence texts below.",
      files: [],
    },
  ];
  // A patched copy must say it is patched: a new vendored crate needs its entry above.
  for (const v of rust.vendored) {
    if (!assets.some((a) => a.note && a.note.startsWith(`vendor/${v.name}:`))) {
      throw new Error(`vendor/${v.name} has no entry in collectAssets saying what was changed`);
    }
  }
  return assets.sort((a, b) => cmp(a.name, b.name));
}

// ---------------------------------------------------------------- rendering

/** Group items' licence files into distinct texts, each listing who uses it. */
function groupTexts(items) {
  const groups = new Map();
  for (const item of items) {
    for (const f of item.files) {
      const h = sha(f.text);
      if (!groups.has(h)) groups.set(h, { hash: h, text: f.text, users: [] });
      groups.get(h).users.push({ who: `${item.name}${item.version ? ` ${item.version}` : ""}`, file: f.name });
    }
  }
  const list = [...groups.values()];
  for (const g of list) g.users.sort((a, b) => cmp(a.who, b.who) || cmp(a.file, b.file));
  list.sort((a, b) => cmp(a.users[0].who, b.users[0].who) || cmp(a.users[0].file, b.users[0].file) || cmp(a.hash, b.hash));
  return list;
}

const anchor = (prefix, i) => `${prefix}-text-${i + 1}`;

// How to recognise a standard licence's text, to point a crate with no licence file at a copy.
const STANDARD_TEXTS = [
  ["Apache-2.0", (t) => /Apache License\s+Version 2\.0/.test(t) && /TERMS AND CONDITIONS/.test(t)],
  ["BSD-3-Clause", (t) => /Redistribution and use in source and binary forms/.test(t) && /Neither the name/.test(t)],
  ["BSL-1.0", (t) => /Boost Software License - Version 1\.0/.test(t)],
  ["CC0-1.0", (t) => /CC0 1\.0 Universal/.test(t)],
  ["MIT", (t) => /Permission is hereby granted, free of charge/.test(t) && !/Apache License/.test(t)],
  ["MPL-2.0", (t) => /Mozilla Public License Version 2\.0/.test(t)],
  ["Zlib", (t) => /provided 'as-is'/.test(t) && /plainly marked as such/.test(t)],
];

/** The most widely shared text that `matches`, as a canonical copy (first on a tie). */
function canonical(groups, matches) {
  let best = null;
  groups.forEach((g, i) => {
    if (matches(g.text) && (!best || g.users.length > best.n)) best = { n: g.users.length, i };
  });
  return best;
}

function renderTexts(lines, groups, prefix) {
  groups.forEach((g, i) => {
    const files = [...new Set(g.users.map((u) => u.file))].sort();
    lines.push(`<a id="${anchor(prefix, i)}"></a>`);
    lines.push(`### ${prefix === "rust" ? "Rust" : "npm"} text ${i + 1}: ${files.join(", ")}`);
    lines.push("");
    lines.push(`Used by: ${g.users.map((u) => (files.length > 1 ? `${u.who} (${u.file})` : u.who)).join(", ")}.`);
    lines.push("");
    lines.push(fence(g.text));
    lines.push("");
  });
}

function renderWithoutFiles(lines, items, groups, prefix) {
  const bare = items.filter((c) => c.files.length === 0);
  if (!bare.length) return;
  const label = prefix === "rust" ? "Rust" : "npm";
  const byLicence = new Map();
  for (const c of bare) {
    if (!byLicence.has(c.license)) byLicence.set(c.license, []);
    byLicence.get(c.license).push(`${c.name} ${c.version}`);
  }
  const ids = new Set();
  for (const lic of byLicence.keys()) {
    for (const id of lic.split(/[\s()/]+|\bOR\b|\bAND\b|\bWITH\b/)) if (id) ids.add(id);
  }
  lines.push(`### ${prefix === "rust" ? "Crates" : "Packages"} that ship no licence file`);
  lines.push("");
  lines.push(
    "These packages state their licence in their manifest but include no licence file, so the standard text " +
      "of the named licence applies (https://spdx.org/licenses/). Where another package here ships that " +
      "standard text, it is linked; an MIT copy's copyright line is then read as naming the package's own authors.",
  );
  lines.push("");
  const refs = [];
  for (const [id, matches] of STANDARD_TEXTS) {
    if (!ids.has(id)) continue;
    const c = canonical(groups, matches);
    if (c) refs.push(`- ${id}: [${label} text ${c.i + 1}](#${anchor(prefix, c.i)})`);
  }
  if (refs.length) {
    lines.push("Standard texts:");
    lines.push("");
    lines.push(...refs);
    lines.push("");
  }
  for (const lic of [...byLicence.keys()].sort()) {
    lines.push(`- **${lic}**: ${byLicence.get(lic).sort().join(", ")}`);
  }
  lines.push("");
}

function renderMarkdown(rust, npm, assets) {
  const rustGroups = groupTexts(rust.crates);
  const npmGroups = groupTexts(npm);
  const lines = [];
  lines.push("<!--");
  lines.push(`  Generated by ${SCRIPT_REL}. Do not edit by hand.`);
  lines.push(`  Regenerate with: node ${SCRIPT_REL}`);
  lines.push(`  Check it is current with: node ${SCRIPT_REL} --check`);
  lines.push("-->");
  lines.push("");
  lines.push("# Third-party notices");
  lines.push("");
  lines.push(
    "jEngine - its runtimes, editor, builder and server - includes the third-party software listed here. " +
      `This file covers ${rust.crates.length} Rust crates, ${npm.length} npm packages and ${assets.length} ` +
      "bundled assets or vendored components, followed by each distinct licence text they ship, printed once.",
  );
  lines.push("");
  lines.push("- [Bundled assets and vendored code](#bundled-assets-and-vendored-code)");
  lines.push("- [Rust crates](#rust-crates)");
  lines.push("- [npm packages (editor)](#npm-packages-editor)");
  lines.push("- [Rust licence texts](#rust-licence-texts)");
  lines.push("- [npm licence texts](#npm-licence-texts)");
  lines.push("");

  lines.push("## Bundled assets and vendored code");
  lines.push("");
  for (const a of assets) {
    lines.push(`### ${a.name}`);
    lines.push("");
    lines.push(`- Licence: ${a.license}`);
    if (a.repository) lines.push(`- Source: ${a.repository}`);
    lines.push("");
    lines.push(a.detail);
    lines.push("");
    for (const f of a.files) {
      lines.push(`Licence text (\`${f.name}\`):`);
      lines.push("");
      lines.push(fence(f.text));
      lines.push("");
    }
  }

  const table = (items) => {
    lines.push("| Name | Version | Licence | Repository |");
    lines.push("| --- | --- | --- | --- |");
    for (const c of items) lines.push(`| ${cell(c.name)} | ${cell(c.version)} | ${cell(c.license)} | ${cell(c.repository)} |`);
    lines.push("");
  };

  lines.push("## Rust crates");
  lines.push("");
  lines.push(
    `Every crate in the normal and build dependency graph of ${SHIPPED_CRATES.map((c) => `\`${c}\``).join(", ")}, ` +
      "on every target platform, from the workspace's Cargo.lock. The workspace's own crates are not listed.",
  );
  lines.push("");
  table(rust.crates);

  lines.push("## npm packages (editor)");
  lines.push("");
  lines.push("The editor's production dependencies (`editor/package.json` `dependencies`) and theirs, as installed, plus the Svelte runtime its compiler builds the editor against (`svelte`, `clsx`, `esm-env`).");
  lines.push("");
  table(npm);

  lines.push("## Rust licence texts");
  lines.push("");
  renderTexts(lines, rustGroups, "rust");
  renderWithoutFiles(lines, rust.crates, rustGroups, "rust");

  lines.push("## npm licence texts");
  lines.push("");
  renderTexts(lines, npmGroups, "npm");
  renderWithoutFiles(lines, npm, npmGroups, "npm");

  return { text: lines.join("\n").replace(/\n+$/, "") + "\n", rustGroups, npmGroups };
}

function renderJson(rust, npm, assets) {
  const strip = ({ name, version, license, repository }) => ({ name, version, license, repository });
  const data = {
    rust: rust.crates.map(strip),
    npm: npm.map(strip),
    assets: assets.map(({ name, license, repository, note }) => (note ? { name, license, repository, note } : { name, license, repository })),
  };
  // One entry a line: compact, and a lockfile change diffs as the lines it touched.
  const section = (key) => {
    const rows = data[key].map((e) => `    ${JSON.stringify(e)}`);
    return `  ${JSON.stringify(key)}: [${rows.length ? `\n${rows.join(",\n")}\n  ` : ""}]`;
  };
  return `{\n${["rust", "npm", "assets"].map(section).join(",\n")}\n}\n`;
}

// ---------------------------------------------------------------- main

function main() {
  const check = process.argv.includes("--check");
  const rust = collectRust();
  const npm = collectNpm();
  const assets = collectAssets(rust);
  const md = renderMarkdown(rust, npm, assets);
  const json = renderJson(rust, npm, assets);
  const outputs = [
    [MD_PATH, md.text],
    [JSON_PATH, json],
  ];
  if (check) {
    // A Windows checkout with core.autocrlf has CRLF on disk; what is written is always LF.
    const onDisk = (p) => readFileSync(p, "utf8").replace(/\r\n/g, "\n");
    const stale = outputs.filter(([p, text]) => !existsSync(p) || onDisk(p) !== text);
    for (const [p] of stale) console.error(`stale: ${toPosix(relative(ROOT, p))} - run node ${SCRIPT_REL}`);
    if (stale.length) process.exit(1);
    console.log("third-party notices are current");
    return;
  }
  for (const [p, text] of outputs) {
    mkdirSync(dirname(p), { recursive: true });
    writeFileSync(p, text, "utf8");
  }
  const bareRust = rust.crates.filter((c) => c.files.length === 0).length;
  const bareNpm = npm.filter((c) => c.files.length === 0).length;
  console.log(
    `${rust.crates.length} crates (${md.rustGroups.length} distinct texts, ${bareRust} with no licence file), ` +
      `${npm.length} npm packages (${md.npmGroups.length} distinct texts, ${bareNpm} with none), ${assets.length} assets`,
  );
}

main();
