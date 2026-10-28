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

## Branches

- **The default branch** (`master` in the development repository, `main`
  in the public one) is what releases come from. Nothing is committed to it
  directly.
- **New work goes on a branch** made from the default branch, named for
  what it does: `feat/<what>`, `fix/<what>`, `docs/<what>` or
  `chore/<what>`. It comes back through a pull request, merged with a merge
  commit. One branch is one change.
- **A release has its own branch**, `release-<version>`, made from the
  default branch when the release is prepared. Nothing is committed to it
  directly either. A fix for the release goes on a branch made from it and
  comes back to it through a pull request. When the release is out, the
  release branch is merged into the default branch, so the default branch
  has everything that shipped.

## Versions

Versions are `MAJOR.MINOR.PATCH`:

- **MAJOR** for a whole new generation of the engine.
- **MINOR** for new features.
- **PATCH** for fixes and improvements to what is already there.

The engine has one version, and it lives in the workspace `version` in
`Cargo.toml`. The engine's own crates follow that version. Each of these
carries a copy of the same number:

- the editor: `editor/package.json` and `editor/src-tauri/tauri.conf.json`
- the website: `website/package.json`

**A package has a version of its own** (since 0.2.0). It moves when the
package changes, by the same rules (a fix is a patch, a feature a minor),
and stays where it was when the package did not change. It is written in
the package's `Cargo.toml` (never `version.workspace = true`) and, when the
package has one, in its `package.toml` as well. A release checks them with
`tools/release/package_versions.py <the last release branch>`, which fails
on a package that changed and kept its number.

The `versions_agree` test fails if a copy of the engine's version differs,
if a package's `package.toml` disagrees with its `Cargo.toml`, or if a
package's `Cargo.toml` takes the workspace version. A package's
`engine_version` is a different number again: the oldest engine the package
works with, which can be older than the engine but never newer.

A release, in order:

1. Make `release-<version>` from the default branch.
2. Give the version's `CHANGELOG.md` section its date, through a pull
   request into the release branch.
3. Tag `v<version>`; the tag has to match the workspace version. The release
   workflow publishes the release with that section of the changelog as its
   notes.
4. Merge the release branch into the default branch.
5. On a branch, move the default branch to the next patch version
   (everything listed above, plus the lock files) and open a new
   `## [<version>] - Unreleased` section in the changelog. If new features
   land before the next release, that release becomes the next minor
   version instead.

## Pull requests

- Keep a pull request to one change, with a description of what and why.
- Add or update tests; CI must pass.
- Add a line to `CHANGELOG.md` under the unreleased version for anything a
  user would notice.
- By contributing you agree that your contribution is licensed under the MIT
  licence of this repository.
