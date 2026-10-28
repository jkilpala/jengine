# jEngine documentation — what is what

Not every document here describes how the engine works today. Some are design
records from work that shipped months ago, and reading one as current is how
you end up implementing against a renderer that no longer exists. This index
says which is which.

**The most complete documentation is the website** (`website/`, Astro +
Starlight). Much of it is *generated* — see [Generated, not written](#generated-not-written).

---

## Authoritative — describes the engine as it is

| Document | Covers |
|---|---|
| [`../CLAUDE.md`](../CLAUDE.md) | Agent guide: architecture, build commands, parity hazards, conventions. Read first. |
| [scripting-api.md](scripting-api.md) | The script-facing API. Upstream of the website *and* the machine-readable AI spec. |
| [writing-packages.md](writing-packages.md) | Writing a Rust package: phases, resources, script commands, the two package lists in `project.jengine`. |
| [parity-gaps.md](parity-gaps.md) | Every place the four runtimes disagree, and which ratchet test guards it. Mark a row FIXED in the change that fixes it. |
| [step7-behavior-graph-spec.md](step7-behavior-graph-spec.md) | The `.behavior` graph format. A living spec — the behavior runtime cites it by section. |
| [desktop-platforms.md](desktop-platforms.md) | Linux (built inside WSL from Windows) and macOS (`.app`, on a Mac) builds: what each is, how the runtime finds its game, and what differs by platform. |
| [build-sizes.md](build-sizes.md) | What an empty project costs to ship on web, Windows, Android and Linux under each release profile and `wasm-opt` level, where the bytes go, and what would shrink them. |
| [graphics-backends.md](graphics-backends.md) | Which wgpu backends a build carries, and why that is a per-project setting. |
| [runtime-profiling.md](runtime-profiling.md) | The F3 overlay: what each row means, and how to read the memory numbers. |
| [web-lua-runtime-proposal.md](web-lua-runtime-proposal.md) | Why the editor and web builds share one Fengari VM. Built; kept as the reason. |
| [scripting-performance-lessons.md](scripting-performance-lessons.md) | Measured costs of the scripting designs that were tried. Still the reason the current one looks the way it does. |
| [manual-verification.md](manual-verification.md) | What still needs a human at the editor or a browser. |
| [brand/](brand/README.md) | The visual style guide: palettes, type, the marks, shape and motion, line and pixel icons (a 12-slot indexed ramp that swaps across all six palettes), with tokens, `.gpl` palettes and boards generated from `editor/src/theme.css` by `brand/build_brand.py`. |

## Reviews and plans — current, with a status line each

| Document | Covers |
|---|---|
| [architecture-debt.md](architecture-debt.md) | The structural problems. Read before a large change in those areas. |
| [docs-audit-2026-10.md](docs-audit-2026-10.md) | Oct 2026 audit of the website, the AI spec files, package docs and the project / package templates: what was wrong, what was fixed, what is open. |
| [core-review-2026-09.md](core-review-2026-09.md) | Sep 2026 read of the core crate: bugs, design flaws, optimisation points, each with a status. |
| [engine-audit-2026-09.md](engine-audit-2026-09.md) | Gaps ranked, with progress. |
| [textured-mesh-proposal.md](textured-mesh-proposal.md) | `Mesh2D`: mostly built; says what is left (`set_mesh`). |
| [stable-script-ids-plan.md](stable-script-ids-plan.md) | Script entity ids that are never reused. Planned, not started. |
| [script-namespaces-plan.md](script-namespaces-plan.md) | Script functions in namespaces (`Sprite.set_color`), flat names kept as aliases. Built Sep 2026; kept as the reason. |
| [determinism-proposal.md](determinism-proposal.md) | The same simulation on every platform: what makes Windows, Mac and the browser play a seed differently, measured, and a phased plan. Proposed Oct 2026, not built. |
| [mobile-platforms-proposal.md](mobile-platforms-proposal.md) | Native Android, then iOS: the phases, how an APK is built, and what the emulator found. Phase 1 built Sep 2026 on `mobile-android`. |
| [feature-backlog.md](feature-backlog.md) | What is not built yet. Shipped items move to [shipped.md](shipped.md). |
| [editor-tooling-plan.md](editor-tooling-plan.md) | Editor tools (Lua debugger, curve editor, sheet slicer, weight painting, PSD import) and Linux / macOS / iOS builds, split out of the Oct 2026 comparison for a session of their own. Not started. |
| [random-ideas.md](random-ideas.md) | Unfiltered ideas. No commitment implied. |

## Historical

[archive/](archive/) — design and execution plans for work that has since
shipped, kept because source comments or the website's changelog generator
cite them. Accurate about *intent at the time*, frequently wrong about how the
engine works now. Do not implement against them.

Removed on 2026-09-26, and in git history before that date: the four
tutorials (they taught a namespaced script API that native does not bind),
the April scripting benchmark baselines and their `projects/scripting-perf`
harness, ten archived plans nothing referenced, and six projects that were
stale or duplicated by newer ones (two early RPG prototypes and a scale
test, superseded by later games; `render-test`, `native-motion-demo` and
`sort-layers-demo`, by the test lab).

---

## Generated, not written

`website/` regenerates these from source with `npm run sync` — edit the
source, not the page:

| Page | Generated from |
|---|---|
| `api-reference/scripting-api.mdx` | [scripting-api.md](scripting-api.md) |
| `api-reference/components.mdx` | `crates/jengine-core/src/ecs/components.rs` + `scene/format.rs` |
| `features/overview.mdx` | `website/scripts/extract-features.ts` |
| `changelog/roadmap.mdx` | the phase list |
| `public/ai/*` (AI spec, scene schema, LLM context) | [scripting-api.md](scripting-api.md) + `components.rs` |

The website's prose pages (getting started, guides, architecture) *are*
hand-written, and they are the ones that drift.

There is no generated reference for **asset formats** (`.scene`, `.prefab`,
`.atlas`, `.tilemap`, `.data`, `.dataschema`, `.gui`, `.input`, `.behavior`,
`.particle`, `.anim`) — the one genuine gap in the reference set. See
`engine-audit-2026-09.md` §3.
