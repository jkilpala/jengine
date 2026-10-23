# jEngine editor

Tauri 2 + Svelte 5 + TypeScript. The game view is the
`jengine-editor-viewport` WASM module, which runs play mode with the same
Fengari Lua VM that web builds ship.

```powershell
./run-editor.ps1        # from the repo root: rebuilds the viewport, then `cargo tauri dev`
npm test                # vitest, for the logic that has no DOM (src/**/*.test.ts)
npx svelte-check        # types
```

Where things are:

- `src/App.svelte` - the shell: docking layout, global shortcuts, menus.
- `src/lib/` - one component per panel or asset editor; `src/lib/stores/`
  holds editor state, `src/lib/api/` wraps the Tauri commands and holds the
  logic worth testing.
- `src/lib/lua-runtime.ts` - the play-mode Lua bridge. A script global missing
  from its `BRIDGE_FUNCTIONS` is `nil` in the editor; see "Parity" in the
  repo's `CLAUDE.md`.
- `src/lib/api/playKeys.ts` - which keys the running game owns.
- `src-tauri/` - the Rust side: commands, the builder bridge, window setup.

Read the repo root's `CLAUDE.md` before changing anything; it covers the
rebuild rules (what needs a viewport rebuild and what does not) and the
parity hazards.
