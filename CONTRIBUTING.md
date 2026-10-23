# Contributing to jEngine

Thank you for helping. Bug reports, fixes, documentation and new engine
features are all welcome.

## Before you start

- **Bugs:** open an issue with the bug template - which runtime (native,
  editor, web, Android), what you did, what you expected, and the smallest
  scene or script that shows it.
- **Larger changes** - a new subsystem, a change to the script API or a file
  format - open an issue first, so the design can be agreed before you spend
  time on it.
- Security problems go to [SECURITY.md](SECURITY.md), not to a public issue.

## Building and testing

The [installation guide](https://jengine.hirakame.com/getting-started/installation/)
covers the tools. The checks a change should pass:

```sh
cargo check --workspace
cargo test --workspace -j 4     # -j 4: the test binaries are large to link
npm test --prefix editor
```

On Windows, `./focused-test.ps1 -Tests <your_tests>` is the quick run: it
compiles the native runtime and both browser hosts, then runs the core tests,
the cross-runtime guards and the GPU goldens in a few minutes.

## Things this codebase cares about

- **Every runtime.** A script function exists in four places - the native
  runtime (Luau), the editor viewport and the web build (both Fengari), and
  the JavaScript preamble. A function wired in one and forgotten in another
  is the most common bug here, and the `*_parity` and `*_reach_every_runtime`
  tests exist to catch it. Read `.github/skills/parity-expert/SKILL.md` before
  adding a script-facing function, and document it in `docs/scripting-api.md`.
- **A test scene for every feature.** A new engine feature gets a scene in
  `projects/engine-test-lab` (see its README), so it can be compared across
  runtimes.
- **Tests that look.** Rendering changes are checked against pixels by
  `cargo test -p jengine-core --features gpu-wgpu --test render_golden`.
- **Generated docs.** The website's API and component references are
  generated - edit the source (`docs/scripting-api.md`, doc comments) and run
  `npm run sync` in `website/`, never the generated page.
- Conventions: `anyhow::Result` for fallible functions, `log` rather than
  `println!` in library code, asset files with semantic extensions (`.scene`,
  `.prefab`, never `.scene.json`).

## Pull requests

- Keep a pull request to one change, with a description of what and why.
- Add or update tests; CI must pass.
- Add a line to `CHANGELOG.md` under the unreleased version for anything a
  user would notice.
- By contributing you agree that your contribution is licensed under the MIT
  licence of this repository.
