//! Web build — packages the WASM runtime + HTML + assets for web deployment.

use std::path::Path;
use std::process::Command;

/// The Fengari runtime bundled from `editor/src/lib/lua-runtime.ts` — the
/// same source the editor loads. Captured at compile time, so it is only the
/// fallback for an engine distribution with no editor sources on disk;
/// `lua_runtime_source` prefers (and refreshes) the file itself.
const LUA_RUNTIME_JS: &str = include_str!("../assets/lua-runtime.js");

/// Generate the HTML template, substituting the game title.
/// Render the project's extra `<script>` tags.
///
/// Emitted as **classic** scripts before the module script that boots the
/// wasm, which is the same arrangement `lua-runtime.js` relies on: a classic
/// script runs to completion before any deferred module, so whatever globals
/// a library installs are in place before the engine's first frame.
///
/// A entry containing `://` is treated as a URL and left alone; anything else
/// is a path relative to the web root, so a project can ship a vendored copy
/// instead of depending on a CDN at runtime.
fn render_extra_scripts(scripts: &[String]) -> String {
    scripts
        .iter()
        .map(|src| {
            // Only the quote character can break out of the attribute; a URL
            // legitimately contains almost everything else.
            let escaped = src.replace('"', "&quot;");
            format!("  <script src=\"{escaped}\"></script>\n")
        })
        .collect()
}

/// `portal`: the web portal's `<head>` tags (`web_platform::head_tags`), its
/// SDK and adapter - there because the SDK must load before any game code.
fn html_template(title: &str, extra_scripts: &[String], portal: &str) -> String {
    let extra = render_extra_scripts(extra_scripts);
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no, viewport-fit=cover">
  <title>{title}</title>
  <style>
    * {{ margin: 0; padding: 0; }}
    html, body {{ width: 100%; height: 100%; overflow: hidden; background: #000; }}
    /* `touch-action: none` hands every gesture to the game: without it the
       browser claims pans and pinches for scrolling and zooming, and the
       canvas never sees them. `user-select` stops a long press selecting the
       page, and the tap highlight stops iOS flashing a grey box on each tap. */
    canvas {{ display: block; width: 100%; height: 100%; image-rendering: pixelated; image-rendering: crisp-edges;
              touch-action: none; user-select: none; -webkit-user-select: none; -webkit-tap-highlight-color: transparent; }}
  </style>
{portal}</head>
<body>
  <canvas id="jengine-canvas"></canvas>
  <!-- Installs `globalThis.__jengine_lua_compile` (Fengari, a real Lua VM).
       A classic script runs before deferred module scripts, so the hook is
       in place before the wasm compiles the first script. Remove it and the
       runtime silently falls back to the Lua->JS transpiler. -->
  <script src="./lua-runtime.js"></script>
{extra}  <script type="module">
    import init from './jengine_web.js';
    init();
  </script>
</body>
</html>
"#,
        title = title,
        portal = portal
    )
}

/// Collect every `.lua` / `.luau` / `.js` under `dir`, as paths relative to `base`
/// (i.e. relative to `assets/`, the form scripts `require` by).
fn collect_scripts(base: &Path, dir: &Path, out: &mut Vec<String>) {
    // `luau` as well: native resolves `require` and `extends` to it, so a
    // manifest without it left those modules missing on the web.
    collect_by_extension(base, dir, &["lua", "luau", "js"], out);
}

/// Every `.skelanim` and `.rig` under `assets`, inlined:
/// `{ "clips": { path: clip }, "rigs": { path: rig } }`.
///
/// Native loads every clip it finds under `assets/` and the editor pushes
/// every one in the project; a browser can walk nothing, so a web build
/// loaded **none** and every `SkeletalPlayer` held its bind pose until Sep
/// 2026. Inlined for the reason the script manifest is - one request - and
/// all of them, because `play_animation` can name any clip at any moment and
/// nothing can be fetched mid-frame. Rigs ride along since Oct 2026, for the
/// same reason: `Skeleton.spawn_rig` names one at any moment.
pub(crate) fn skelanim_manifest(assets: &Path) -> serde_json::Value {
    serde_json::json!({
        "clips": inline_json(assets, "skelanim"),
        "rigs": inline_json(assets, "rig"),
    })
}

/// The bundle of small assets a web build ships beside `assets/`, read by the
/// web runtime before it fetches anything else.
pub const ASSET_BUNDLE_FILE: &str = "asset_bundle.json";

/// The asset types that go into the bundle: every small file the web runtime
/// fetches by name. Scenes stay out (a player downloads only the ones they
/// visit), as do scripts and skeletal clips, which have manifests of their own.
const BUNDLED_EXTENSIONS: &[&str] =
    &["prefab", "material", "shader", "gui", "data", "font", "behavior", "sortlayers", "anim", "atlas"];

/// A file larger than this is left to be fetched on its own.
const BUNDLE_FILE_LIMIT: u64 = 1024 * 1024;

/// Every small asset the web runtime fetches by name, in one file:
/// `{ "files": { path: content } }`, the path relative to `assets/` and the
/// content the file's JSON (or its text, if it is not JSON).
///
/// A web game pays a round trip per file, and these are many and small - a
/// showcase build fetched 115 prefabs one request each before its first
/// frame. The files stay beside it too, for anything that asks for one by URL.
pub(crate) fn asset_bundle(assets: &Path) -> serde_json::Value {
    let mut paths = Vec::new();
    collect_by_extension(assets, assets, BUNDLED_EXTENSIONS, &mut paths);
    paths.sort();
    let mut files = serde_json::Map::new();
    for rel in paths {
        let path = assets.join(&rel);
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > BUNDLE_FILE_LIMIT) {
            continue;
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let content = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
                files.insert(rel, content);
            }
            Err(e) => log::warn!("asset bundle: reading {rel}: {e}"),
        }
    }
    serde_json::json!({ "files": files })
}

/// The JSON asset types a release web build minifies.
const MINIFIED_EXTENSIONS: &[&str] = &[
    "scene", "prefab", "material", "shader", "gui", "data", "dataschema", "font", "behavior", "sortlayers", "anim",
    "atlas", "tilemap", "particle", "skelanim", "rig", "json",
];

/// Rewrite every JSON asset under `assets` without its whitespace: the same
/// data, about half the bytes before compression (the showcase's scenes went
/// from 10.7 MB to 5.9). The project's own files stay as they are - only the
/// build's copies change. Returns the files rewritten and the bytes saved.
pub(crate) fn minify_json_assets(assets: &Path) -> (usize, u64) {
    let mut paths = Vec::new();
    collect_by_extension(assets, assets, MINIFIED_EXTENSIONS, &mut paths);
    let (mut count, mut saved) = (0, 0u64);
    for rel in paths {
        let path = assets.join(&rel);
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        // Not JSON (a `.json` of some other tool's making): left alone.
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { continue };
        let Ok(compact) = serde_json::to_string(&value) else { continue };
        if compact.len() < text.len() && std::fs::write(&path, &compact).is_ok() {
            count += 1;
            saved += (text.len() - compact.len()) as u64;
        }
    }
    (count, saved)
}

/// Every `.<ext>` file under `assets` that parses as JSON, by its path there.
fn inline_json(assets: &Path, ext: &str) -> serde_json::Map<String, serde_json::Value> {
    let mut files = Vec::new();
    collect_by_extension(assets, assets, &[ext], &mut files);
    files.sort();
    let mut out = serde_json::Map::new();
    for rel in files {
        match std::fs::read_to_string(assets.join(&rel)).map(|t| serde_json::from_str::<serde_json::Value>(&t)) {
            Ok(Ok(v)) => {
                out.insert(rel, v);
            }
            Ok(Err(e)) => log::warn!("skelanim manifest: {rel} is not JSON: {e}"),
            Err(e) => log::warn!("skelanim manifest: reading {rel}: {e}"),
        }
    }
    out
}

/// Every file under `dir` with one of `exts`, as a `/`-separated path
/// relative to `base`.
fn collect_by_extension(base: &Path, dir: &Path, exts: &[&str], out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_by_extension(base, &path, exts, out);
            continue;
        }
        let wanted = path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| exts.iter().any(|x| e.eq_ignore_ascii_case(x)));
        if !wanted {
            continue;
        }
        if let Some(rel) = path.strip_prefix(base).ok().and_then(|r| r.to_str()) {
            out.push(rel.replace('\\', "/"));
        }
    }
}

/// Read the game title from project.jengine, falling back to a default.
fn read_project_title(project_path: &Path) -> String {
    let project_file = project_path.join("project.jengine");
    if let Ok(contents) = std::fs::read_to_string(&project_file)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&contents)
        && let Some(title) = val
            .get("settings")
            .and_then(|s| s.get("title"))
            .and_then(|t| t.as_str())
        && !title.is_empty()
    {
        return title.to_string();
    }
    "jEngine Game".to_string()
}

/// Extra browser scripts the project wants in `index.html`.
///
/// From `settings.web_scripts` in `project.jengine`. This is how a game reaches
/// a JS library the engine knows nothing about — a networking SDK, an
/// analytics shim, a payment widget. The engine calls into whatever globals
/// they install through the interop contract in `jengine-net`'s `js_transport`.
///
/// Returns the entries in order; the order is the load order, which matters
/// when one script depends on another.
fn read_web_scripts(project_path: &Path) -> Vec<String> {
    let project_file = project_path.join("project.jengine");
    let Ok(raw) = std::fs::read_to_string(&project_file) else {
        return Vec::new();
    };
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return Vec::new();
    };
    let Some(list) = doc
        .get("settings")
        .and_then(|s| s.get("web_scripts"))
        .and_then(|v| v.as_array())
    else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|v| match v.as_str() {
            Some(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
            _ => {
                log::warn!("settings.web_scripts entry is not a non-empty string; ignoring");
                None
            }
        })
        .collect()
}

/// Read the physics backend from project.jengine ("rapier2d" or "none").
fn read_physics_backend(project_path: &Path) -> String {
    let project_file = project_path.join("project.jengine");
    if let Ok(contents) = std::fs::read_to_string(&project_file)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&contents)
        && let Some(backend) = val
            .get("settings")
            .and_then(|s| s.get("physics"))
            .and_then(|p| p.get("backend"))
            .and_then(|b| b.as_str())
        && !backend.is_empty()
    {
        return backend.to_string();
    }
    "rapier2d".to_string()
}

/// Build a Web distributable for the given project.
///
/// This will:
/// 1. Run `wasm-pack build` on jengine-web
/// 2. Copy generated WASM + JS glue to `output_path/`
/// 3. Generate `index.html`
/// 4. Copy all assets to `output_path/assets/`
///
/// `platform`: the web portal to build for, over the project's
/// `settings.web_platform` (`web_platform`); `None` keeps the project's.
/// `logging`: whether the game writes its info lines to the console
/// (`set_logging`); warnings and errors are written either way.
#[allow(clippy::too_many_arguments)]
pub fn build_web(
    engine_path: &Path,
    project_path: &Path,
    output_path: &Path,
    release: bool,
    generate_report: bool,
    wasm_lto: bool,
    wasm_opt_size: bool,
    wasm_single_codegen: bool,
    wasm_opt: crate::wasm_opt::WasmOpt,
    debug_overlay: bool,
    logging: bool,
    platform: Option<crate::web_platform::WebPlatform>,
) -> anyhow::Result<()> {
    // Absolute before anything derives from them: wasm-pack resolves a
    // relative `--out-dir` against the crate it builds, not the working
    // directory, so a relative project path put the output inside itself.
    let engine_path = &std::path::absolute(engine_path)?;
    let project_path = &std::path::absolute(project_path)?;
    let output_path = &std::path::absolute(output_path)?;
    let profile_name = if release { "release" } else { "debug" };
    log::info!(
        "Building Web target ({}): engine={:?} project={:?} -> {:?}",
        profile_name,
        engine_path,
        project_path,
        output_path
    );

    // 1. Run wasm-pack build (in the engine workspace)
    crate::progress::step(&format!(
        "Building WASM with wasm-pack ({profile_name})..."
    ));
    let wasm_profile = if release { "--release" } else { "--dev" };

    // A project with Rust packages builds a generated crate that links them
    // (`packages::generate_project_web`); one without builds the stock
    // `jengine-web`, exactly as before packages ran on the web.
    let packages = crate::packages::resolve_packages(project_path)
        .map_err(|e| anyhow::anyhow!("resolving the project's packages: {e:#}"))?;
    let project_crate =
        crate::packages::generate_project_web(project_path, engine_path, &packages)?;
    let package_names: Vec<&str> = packages
        .iter()
        .filter(|p| p.package_type == jengine_core::package::manifest::PackageType::Full)
        .map(|p| p.name.as_str())
        .collect();
    let (wasm_crate, wasm_pkg) = match &project_crate {
        Some(manifest) => {
            let dir = manifest
                .parent()
                .expect("generated manifest has a parent")
                .to_path_buf();
            crate::progress::step(&format!(
                "Linking {} project package(s) into the web runtime: {}",
                package_names.len(),
                package_names.join(", ")
            ));
            // Its own output directory, beside the crate: the stock build's
            // `target/wasm-pkg` is shared by every project.
            let pkg = dir.join("pkg");
            (dir, pkg)
        }
        None => (
            engine_path.join("crates").join("jengine-web"),
            engine_path.join("target").join("wasm-pkg"),
        ),
    };

    // Check if physics is enabled in project settings
    let physics_backend = read_physics_backend(project_path);
    let physics_enabled = physics_backend != "none";
    if !physics_enabled {
        log::info!("Physics disabled in project settings — excluding rapier2d/parry2d from WASM");
    }

    // Compose the feature set explicitly rather than leaning on defaults:
    // physics is a project setting, and so are the graphics backends, so the
    // build only carries what this project asked for.
    let backends = crate::graphics::backends_for_platform(project_path, "web")
        .unwrap_or_else(|| vec!["webgpu".to_string()]);
    let mut features = crate::graphics::features_for(&backends);
    if features.is_empty() {
        // A project that names only unknown backends would otherwise build a
        // renderer-less wasm that fails at adapter request.
        log::warn!("no usable web graphics backend selected - falling back to WebGPU");
        features.push("backend-webgpu".to_string());
    }
    if physics_enabled {
        features.push("physics".to_string());
    }
    // The stock crate's own module-start function. A generated crate has one
    // of its own and must not get this one too: wasm-bindgen allows one.
    if project_crate.is_none() {
        features.push("standalone".to_string());
    }
    // The engine features (`engine_features`): on the stock crate's command
    // line, since `--no-default-features` empties them; a generated crate
    // names them on its own `jengine-core` dependency.
    let engine = crate::engine_features::for_project(project_path)?;
    if project_crate.is_none() {
        features.extend(engine.host_features());
    }
    crate::progress::step(&engine.summary());
    log::info!("Web graphics backends: {}", backends.join(", "));

    let mut cmd = Command::new("wasm-pack");
    cmd.args(["build", "--target", "web", wasm_profile, "--out-dir"])
        .arg(&wasm_pkg)
        .current_dir(&wasm_crate);
    // Pins the emitted files to `jengine_web.js` / `jengine_web_bg.wasm`,
    // which `index.html` imports, whatever the crate is called.
    cmd.args(["--out-name", "jengine_web"]);

    // A release module is optimised at the project's level afterwards; a dev
    // build is never optimised (wasm-pack's own rule). wasm-pack's own flags
    // go before the cargo ones: from the first flag it does not know, wasm-pack
    // hands everything to `cargo build`, and cargo refuses `--no-opt`.
    let opt_plan = release.then(|| crate::wasm_opt::Plan::new(wasm_opt));
    if let Some(plan) = &opt_plan {
        cmd.args(plan.wasm_pack_args());
    }

    cmd.args(["--no-default-features", "--features", &features.join(",")]);

    // Apply WASM optimization env vars (release only)
    if release {
        if wasm_lto {
            cmd.env("CARGO_PROFILE_RELEASE_LTO", "true");
            log::info!("WASM optimization: LTO enabled");
        }
        if wasm_opt_size {
            cmd.env("CARGO_PROFILE_RELEASE_OPT_LEVEL", "z");
            log::info!("WASM optimization: opt-level = 'z' (size)");
        }
        if wasm_single_codegen {
            cmd.env("CARGO_PROFILE_RELEASE_CODEGEN_UNITS", "1");
            log::info!("WASM optimization: codegen-units = 1");
        }
    }

    // Streamed rather than inherited: this is the long part of a web build,
    // and its output is what the editor's Build panel shows.
    let status = crate::progress::run_streaming(&mut cmd, "wasm-pack")?;

    if !status.success() {
        anyhow::bail!("wasm-pack build failed with status: {status}");
    }
    if let Some(plan) = &opt_plan {
        plan.optimize(&wasm_pkg.join("jengine_web_bg.wasm"))?;
    }

    // 2. Create output directory and copy WASM artifacts
    std::fs::create_dir_all(output_path)?;

    for entry in std::fs::read_dir(&wasm_pkg)? {
        let entry = entry?;
        let path = entry.path();
        if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && (ext == "wasm" || ext == "js")
        {
            let dest = output_path.join(entry.file_name());
            std::fs::copy(&path, &dest)?;
            log::info!("Copied {}", dest.display());
        }
    }

    // 2.b Ship the Lua runtime.
    //
    // Deployed builds run game Lua through the same Fengari VM the editor
    // uses, so "it works in the editor" means something. Without this file
    // the wasm falls back to the line-based Lua->JS transpiler, which
    // accepts a narrower language than either the editor or native Luau.
    // See docs/web-lua-runtime-proposal.md.
    let lua_runtime = lua_runtime_source(engine_path);
    std::fs::write(output_path.join("lua-runtime.js"), lua_runtime.as_bytes())?;
    log::info!("Copied lua-runtime.js ({} KB)", lua_runtime.len() / 1024);

    // 3. Generate index.html with game title
    let title = read_project_title(project_path);
    let extra_scripts = read_web_scripts(project_path);
    if !extra_scripts.is_empty() {
        crate::progress::step(&format!(
            "Including {} project script(s) in index.html",
            extra_scripts.len()
        ));
    }
    // 3.b The web portal: its SDK in the page, and the adapter the engine
    //     talks to beside it (`web_platform`).
    let (project_platform, game_ready) = crate::web_platform::read_settings(project_path);
    let platform = platform.unwrap_or(project_platform);
    let adapter_path = output_path.join(crate::web_platform::ADAPTER_FILE);
    let wasm_file = "jengine_web_bg.wasm";
    let wasm_bytes = std::fs::metadata(output_path.join(wasm_file)).map(|m| m.len()).unwrap_or(0);
    match crate::web_platform::adapter_js(platform, game_ready, wasm_file, wasm_bytes) {
        Some(js) => {
            std::fs::write(&adapter_path, js)?;
            crate::progress::step(&format!(
                "Web portal: {} (game ready on {})",
                platform.id(),
                game_ready.id().replace('_', " ")
            ));
        }
        // A folder built for a portal before: its adapter must not linger.
        None => {
            let _ = std::fs::remove_file(&adapter_path);
        }
    }
    let html_path = output_path.join("index.html");
    std::fs::write(
        &html_path,
        html_template(&title, &extra_scripts, &crate::web_platform::head_tags(platform)),
    )?;
    log::info!("Generated {}", html_path.display());

    // 4. Copy project.jengine with entry_scene injected for the web runtime
    let project_file = project_path.join("project.jengine");
    if project_file.exists() {
        let raw = std::fs::read_to_string(&project_file)?;
        let mut doc: serde_json::Value = serde_json::from_str(&raw)
            .unwrap_or_else(|_| serde_json::json!({}));

        // Development overlays are a build option, not a project property
        // - the runtime reads this key to decide whether F3 / F12 do
        // anything. Stamped before `settings` is borrowed below.
        crate::set_debug_overlay(&mut doc, debug_overlay);
        if !debug_overlay {
            log::info!("Debug overlay disabled for this build (F3 / F12 inert)");
        }
        crate::set_logging(&mut doc, logging);
        if logging && release {
            log::warn!("Logs are on in a release build: players see every info line in the browser console");
        } else if !logging {
            log::info!("Logs off for this build (warnings and errors still reach the console)");
        }

        // Auto-detect entry_scene if not explicitly set
        let settings = doc.get_mut("settings")
            .and_then(|s| s.as_object_mut());
        if let Some(settings) = settings {
            let has_entry = settings.get("entry_scene")
                .and_then(|v| v.as_str())
                .is_some_and(|s| !s.is_empty());
            if !has_entry {
                // Find first .scene file in assets/scenes/
                let scenes_dir = project_path.join("assets").join("scenes");
                if scenes_dir.exists()
                    && let Some(scene_file) = std::fs::read_dir(&scenes_dir)
                        .ok()
                        .and_then(|rd| rd
                            .filter_map(|e| e.ok())
                            .find(|e| e.path().extension().is_some_and(|ext| ext == "scene")))
                {
                    let scene_name = format!("scenes/{}", scene_file.file_name().to_string_lossy());
                    log::info!("Auto-detected entry_scene: {}", scene_name);
                    settings.insert("entry_scene".to_string(), serde_json::Value::String(scene_name));
                }
            }

            // Data-oriented design — list every `.data` table so the web
            // runtime can fetch and seed them for the `get_data` script
            // global (the browser has no filesystem walker). Paths are
            // fetch URLs relative to the web root (e.g. "assets/data/x.data").
            let assets_dir = project_path.join("assets");
            let mut data_tables: Vec<String> = Vec::new();
            collect_data_tables(&assets_dir, &assets_dir, &mut data_tables);
            // A table an asset pack takes arrives with the pack, not at boot.
            let packs = crate::asset_packs::read_settings(project_path);
            data_tables.retain(|t| packs.claimant(t).is_none());
            data_tables.sort();
            log::info!("Listed {} data table(s) for web runtime", data_tables.len());
            settings.insert(
                "data_tables".to_string(),
                serde_json::Value::Array(
                    data_tables
                        .into_iter()
                        .map(serde_json::Value::String)
                        .collect(),
                ),
            );
        }

        let out_json = serde_json::to_string_pretty(&doc)?;
        std::fs::write(output_path.join("project.jengine"), out_json)?;
        log::info!("Wrote project.jengine with entry_scene");
    }

    // 5. Copy assets from the game project
    let source_assets = project_path.join("assets");
    let dest_assets = output_path.join("assets");
    if source_assets.exists() {
        crate::stage::copy_dir_recursive(&source_assets, &dest_assets)?;
        crate::progress::step(&format!("Copied assets to {}", dest_assets.display()));
    }
    // The web runtime resolves `.anim` references itself and never did
    // `.tilemap` or `.particle` ones - an external tilemap loaded empty. A
    // shipped scene carries all three (see `stage::inline_scene_sources`).
    crate::stage::inline_scene_sources(&dest_assets)?;
    crate::stage::write_licenses(output_path)?;

    // 6. Run asset pipeline (atlas packing, definitions, compression)
    //    Always run — even if packing & compression are disabled, atlas definitions
    //    (.atlas files) still need processing and an empty manifest must be written
    //    so the web runtime doesn't 404.
    let mut pipeline_config = crate::asset_pipeline::read_pipeline_config(project_path);
    // A pack's sprites stay out of the atlases (`asset_packs`).
    let packs = crate::asset_packs::read_settings(project_path);
    pipeline_config.atlas.exclude_patterns.extend(crate::asset_packs::atlas_excludes(&packs));
    crate::progress::step("Running asset pipeline...");
    let pipeline_result = crate::asset_pipeline::run_asset_pipeline(output_path, &pipeline_config)?;
    if pipeline_result.atlases_created > 0 {
        log::info!(
            "Asset pipeline: {} atlases ({} sprites packed)",
            pipeline_result.atlases_created,
            pipeline_result.sprites_packed
        );
    }
    if let Some(manifest) = &pipeline_result.manifest {
        crate::asset_pipeline::remap_all_references(
            &output_path.join("assets"),
            manifest,
        )?;
    }

    // 6b. Asset packs out of the game, into `packs/` beside the page - the
    //     default url, so a deployed folder downloads from itself.
    let entry = crate::asset_packs::entry_scene(project_path);
    crate::asset_packs::split(&dest_assets, &output_path.join("packs"), &packs, entry.as_deref())?;

    // 7. Generate prefab manifest (lists all .prefab files for the web runtime)
    {
        let prefabs_dir = dest_assets.join("prefabs");
        let mut prefab_files: Vec<String> = Vec::new();
        if prefabs_dir.exists() {
            for entry in std::fs::read_dir(&prefabs_dir)? {
                let entry = entry?;
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".prefab") {
                    prefab_files.push(format!("prefabs/{}", name));
                }
            }
        }
        let manifest = serde_json::json!({ "prefabs": prefab_files });
        std::fs::write(
            dest_assets.join("prefab_manifest.json"),
            serde_json::to_string_pretty(&manifest)?,
        )?;
        log::info!("Generated prefab manifest: {} prefabs", prefab_files.len());
    }

    // 8. Generate the script manifest, sources included.
    //
    // `require("lib/foo")` resolves synchronously inside the script VM, so
    // every module a script might pull has to be in memory before the first
    // compile - the browser cannot fetch mid-`require`. The editor gets this
    // for free by walking the project directory; a web build has no
    // filesystem.
    //
    // The sources are inlined rather than listed, because the cost that
    // scales here is round trips, not bytes: a list means one request per
    // script, serialized, so 200 scripts on a 120 ms link is ~24 s of dead
    // time before the first frame. Inlined, boot is a single request whatever
    // the script count, and the concatenation compresses slightly better than
    // the files do separately. The trade is that editing one script
    // re-downloads the bundle - fine for something that ships as a unit.
    {
        let mut script_files: Vec<String> = Vec::new();
        collect_scripts(&dest_assets, &dest_assets, &mut script_files);
        script_files.sort();

        let mut sources = serde_json::Map::new();
        let mut bytes = 0usize;
        for rel in &script_files {
            match std::fs::read_to_string(dest_assets.join(rel)) {
                Ok(text) => {
                    bytes += text.len();
                    sources.insert(rel.clone(), serde_json::Value::String(text));
                }
                Err(e) => log::warn!("script manifest: reading {rel}: {e}"),
            }
        }
        let count = sources.len();
        // `scripts` (the path list) stays for older runtimes, which fetch
        // each path individually; `sources` is what current ones read.
        let manifest = serde_json::json!({
            "scripts": script_files,
            "sources": serde_json::Value::Object(sources),
        });
        std::fs::write(
            dest_assets.join("script_manifest.json"),
            serde_json::to_string(&manifest)?,
        )?;
        log::info!("Generated script manifest: {count} script(s), {} KB of source", bytes / 1024);
    }

    // 9. Skeletal clips, inlined - see `skelanim_manifest`.
    {
        let manifest = skelanim_manifest(&dest_assets);
        let count = manifest["clips"].as_object().map_or(0, |m| m.len());
        let rigs = manifest["rigs"].as_object().map_or(0, |m| m.len());
        std::fs::write(dest_assets.join("skelanim_manifest.json"), serde_json::to_string(&manifest)?)?;
        log::info!("Generated skeletal manifest: {count} clip(s), {rigs} rig(s)");
    }

    // 10. A release build's JSON without its whitespace - the project's own
    //     files stay readable; only what ships is minified.
    if release {
        let (count, saved) = minify_json_assets(&dest_assets);
        log::info!("Minified {count} JSON asset(s), {} KB smaller", saved / 1024);
    }

    // 11. The small assets in one file - see `asset_bundle`.
    {
        let bundle = asset_bundle(&dest_assets);
        let count = bundle["files"].as_object().map_or(0, |m| m.len());
        let text = serde_json::to_string(&bundle)?;
        std::fs::write(dest_assets.join(ASSET_BUNDLE_FILE), &text)?;
        log::info!("Generated asset bundle: {count} file(s), {} KB", text.len() / 1024);
    }

    crate::progress::step(&format!("Web build complete: {}", output_path.display()));

    if generate_report {
        crate::report::generate_report(output_path, "web", profile_name, Some(engine_path), Some(&engine))?;
    }

    Ok(())
}

/// Recursively collect every `.data` table under `dir`, pushing fetch URLs
/// relative to `root`'s parent (i.e. prefixed with `assets/`) so the web
/// runtime can fetch them directly (e.g. `assets/data/enemies.data`).
fn collect_data_tables(dir: &Path, assets_root: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_data_tables(&path, assets_root, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("data") {
            if let Ok(rel) = path.strip_prefix(assets_root) {
                let rel = rel.to_string_lossy().replace('\\', "/");
                out.push(format!("assets/{rel}"));
            }
        }
    }
}


/// The Fengari runtime source to ship, rebuilt from TypeScript when stale.
///
/// The bundle is a committed artifact, so editing `lua-runtime.ts` without
/// regenerating it ships a stale VM: a script global added to
/// `BRIDGE_FUNCTIONS` works in the editor (Vite bundles the source directly)
/// and is `nil` in the browser build - the exact editor-vs-web split the
/// Fengari switch existed to end. A build regenerates it rather than hoping
/// someone reads a warning.
///
/// Order of preference:
/// 1. Rebuild with esbuild when the `.ts` is newer than the `.js` (or the
///    `.js` is missing) and the editor's `node_modules` is present.
/// 2. Whatever is on disk - freshly built, or committed and current.
/// 3. The copy embedded at compile time, for an engine distribution that
///    ships without the editor sources.
///
/// Reading from disk matters: `LUA_RUNTIME_JS` is `include_str!`d when this
/// crate compiles, so a rebuild during a build would otherwise not be the
/// thing that ships.
fn lua_runtime_source(engine_path: &Path) -> String {
    let source = engine_path
        .join("editor")
        .join("src")
        .join("lib")
        .join("lua-runtime.ts");
    let bundle = engine_path
        .join("crates")
        .join("jengine-builder")
        .join("assets")
        .join("lua-runtime.js");

    if source.is_file() && bundle_is_stale(&source, &bundle) {
        match rebuild_lua_runtime(engine_path, &bundle) {
            Ok(()) => crate::progress::step("Rebuilt lua-runtime.js from lua-runtime.ts"),
            Err(e) => log::warn!(
                "lua-runtime.ts is newer than lua-runtime.js and rebuilding failed ({e}). This build ships a stale Lua runtime - script globals added since the last bundle will be nil in the browser but fine in the editor. Run ./build-lua-runtime.ps1 manually."
            ),
        }
    }

    match std::fs::read_to_string(&bundle) {
        Ok(text) => text,
        // No checkout to read from - use the copy compiled into this binary.
        Err(_) => LUA_RUNTIME_JS.to_string(),
    }
}

/// True when the bundle is missing or older than its source.
fn bundle_is_stale(source: &Path, bundle: &Path) -> bool {
    let Ok(bundle_meta) = bundle.metadata() else {
        return true;
    };
    let (Ok(src_time), Ok(bundle_time)) = (
        source.metadata().and_then(|m| m.modified()),
        bundle_meta.modified(),
    ) else {
        // Cannot compare - leave the artifact alone rather than rebuilding
        // on every single build.
        return false;
    };
    src_time > bundle_time
}

/// Run the same esbuild command `build-lua-runtime.ps1` uses.
///
/// Needs `editor/node_modules` (esbuild arrives with Vite). `npx` is a shell
/// script on Windows, which `CreateProcess` will not launch directly, so it
/// goes through `cmd /C` there.
fn rebuild_lua_runtime(engine_path: &Path, bundle: &Path) -> anyhow::Result<()> {
    let editor_dir = engine_path.join("editor");
    if !editor_dir.join("node_modules").is_dir() {
        anyhow::bail!("editor/node_modules is missing - run `npm install` in editor/");
    }
    // Fengari's `%` and `^` by Luau's rules first (`patch-fengari.mjs`, a
    // no-op once done): a bundle built from an unpatched `fengari-web` ships
    // a web build whose scripts compute different numbers from native.
    let status = crate::progress::run_streaming(
        Command::new("node").arg("scripts/patch-fengari.mjs").current_dir(&editor_dir),
        "node scripts/patch-fengari.mjs",
    )?;
    if !status.success() {
        anyhow::bail!("patching fengari-web failed ({status})");
    }
    let esbuild_args = [
        "esbuild",
        "src/lib/lua-runtime.ts",
        "--bundle",
        "--format=iife",
        "--minify",
        "--target=es2020",
    ];
    let out_arg = format!("--outfile={}", bundle.display());

    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg("npx").args(esbuild_args).arg(&out_arg);
        c
    } else {
        let mut c = Command::new("npx");
        c.args(esbuild_args).arg(&out_arg);
        c
    };

    let status = crate::progress::run_streaming(cmd.current_dir(&editor_dir), "npx esbuild")?;
    if !status.success() {
        anyhow::bail!("esbuild exited with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod web_script_tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    /// wasm-pack hands everything from the first flag it does not know to
    /// `cargo build`, so a wasm-pack flag after `--no-default-features` reached
    /// cargo, which refused `--no-opt` - every release web build failed.
    #[test]
    fn wasm_packs_own_flags_come_before_the_cargo_ones() {
        let src = include_str!("web.rs");
        let own = src.find("cmd.args(plan.wasm_pack_args())").expect("the optimiser's wasm-pack flags");
        let cargo = src.find("cmd.args([\"--no-default-features\"").expect("the cargo feature flags");
        assert!(own < cargo, "wasm-pack's own flags must be added before the cargo ones");
    }

    fn tmp_project(settings: &str) -> std::path::PathBuf {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "jengine-web-scripts-{}-{}",
            std::process::id(),
            id
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("project.jengine"),
            format!(r#"{{"name":"t","settings":{settings}}}"#),
        )
        .unwrap();
        dir
    }

    #[test]
    fn the_bundle_holds_the_small_assets_and_not_the_scenes() {
        let dir = tmp_project(r#"{}"#);
        let assets = dir.join("assets");
        std::fs::create_dir_all(assets.join("prefabs/deep")).unwrap();
        std::fs::create_dir_all(assets.join("scenes")).unwrap();
        std::fs::write(assets.join("prefabs/deep/crate.prefab"), r#"{ "name": "crate" }"#).unwrap();
        std::fs::write(assets.join("materials.material"), r#"{"shader":"x"}"#).unwrap();
        std::fs::write(assets.join("odd.gui"), "not json").unwrap();
        std::fs::write(assets.join("scenes/main.scene"), r#"{"entities":[]}"#).unwrap();
        std::fs::write(assets.join("big.data"), vec![b' '; BUNDLE_FILE_LIMIT as usize + 1]).unwrap();
        let bundle = asset_bundle(&assets);
        let files = bundle["files"].as_object().unwrap();
        let mut keys: Vec<&str> = files.keys().map(|k| k.as_str()).collect();
        keys.sort();
        // Nested, by its path under `assets/`; a scene and an oversize file left out.
        assert_eq!(keys, vec!["materials.material", "odd.gui", "prefabs/deep/crate.prefab"]);
        assert_eq!(files["prefabs/deep/crate.prefab"]["name"], "crate");
        // A file that is not JSON is carried as its text.
        assert_eq!(files["odd.gui"], "not json");
        // The web runtime asks for it by this name.
        let runtime = include_str!("../../jengine-web/src/lib.rs");
        assert!(runtime.contains(&format!("\"assets/{ASSET_BUNDLE_FILE}\"")), "the web runtime reads no asset bundle");
    }

    #[test]
    fn minifying_keeps_the_data_and_leaves_other_files_alone() {
        let dir = tmp_project(r#"{}"#);
        let assets = dir.join("assets");
        std::fs::create_dir_all(assets.join("scenes")).unwrap();
        let scene = "{\n  \"entities\": [\n    { \"name\": \"a\", \"x\": 1.5 }\n  ]\n}\n";
        std::fs::write(assets.join("scenes/main.scene"), scene).unwrap();
        std::fs::write(assets.join("notes.json"), "not { json").unwrap();
        std::fs::write(assets.join("main.lua"), "local  x = 1\n").unwrap();
        let (count, saved) = minify_json_assets(&assets);
        assert_eq!(count, 1);
        let out = std::fs::read_to_string(assets.join("scenes/main.scene")).unwrap();
        assert_eq!(out, r#"{"entities":[{"name":"a","x":1.5}]}"#);
        assert_eq!(saved as usize, scene.len() - out.len());
        let before: serde_json::Value = serde_json::from_str(scene).unwrap();
        assert_eq!(serde_json::from_str::<serde_json::Value>(&out).unwrap(), before);
        assert_eq!(std::fs::read_to_string(assets.join("notes.json")).unwrap(), "not { json");
        assert_eq!(std::fs::read_to_string(assets.join("main.lua")).unwrap(), "local  x = 1\n");
    }

    #[test]
    fn every_skeletal_clip_is_inlined_into_the_manifest() {
        // A web build could load no `.skelanim` at all: native walks the
        // disk and the editor pushes every clip, and a browser can do
        // neither. The manifest carries all of them, nested ones too.
        let dir = tmp_project(r#"{}"#);
        let assets = dir.join("assets");
        std::fs::create_dir_all(assets.join("animations/deep")).unwrap();
        std::fs::write(assets.join("animations/wave.skelanim"), r#"{"name":"wave","duration":1.0,"tracks":[]}"#).unwrap();
        std::fs::write(assets.join("animations/deep/tail.skelanim"), r#"{"name":"tail","duration":2.0}"#).unwrap();
        std::fs::write(assets.join("animations/walk.anim"), "{}").unwrap();
        std::fs::write(assets.join("animations/broken.skelanim"), "not json").unwrap();
        let m = skelanim_manifest(&assets);
        let clips = m["clips"].as_object().unwrap();
        let mut keys: Vec<&str> = clips.keys().map(|k| k.as_str()).collect();
        keys.sort();
        assert_eq!(keys, vec!["animations/deep/tail.skelanim", "animations/wave.skelanim"]);
        assert_eq!(clips["animations/deep/tail.skelanim"]["duration"], 2.0);
        // Rigs ride in the same file, for `Skeleton.spawn_rig`.
        assert!(m["rigs"].as_object().unwrap().is_empty());
        std::fs::create_dir_all(assets.join("rigs")).unwrap();
        std::fs::write(assets.join("rigs/arm.rig"), r#"{"name":"arm","bones":[{"name":"upper"}]}"#).unwrap();
        let m = skelanim_manifest(&assets);
        assert_eq!(m["rigs"]["rigs/arm.rig"]["bones"][0]["name"], "upper");
        assert_eq!(m["clips"].as_object().unwrap().len(), 2, "rigs are not clips");
    }

    #[test]
    fn a_project_without_the_key_gets_no_extra_scripts() {
        let dir = tmp_project(r#"{"title":"t"}"#);
        assert!(read_web_scripts(&dir).is_empty());
        // And the template is byte-identical to what it always produced.
        assert!(!html_template("t", &[], "").contains("<script src=\"http"));
    }

    #[test]
    fn declared_scripts_are_read_in_order() {
        // Order is load order, and a library that depends on another has to
        // come second.
        let dir = tmp_project(
            r#"{"web_scripts":["https://unpkg.com/@colyseus/sdk/dist/colyseus.js","net-adapter.js"]}"#,
        );
        assert_eq!(
            read_web_scripts(&dir),
            vec![
                "https://unpkg.com/@colyseus/sdk/dist/colyseus.js".to_string(),
                "net-adapter.js".to_string(),
            ]
        );
    }

    #[test]
    fn junk_entries_are_skipped_not_emitted() {
        let dir = tmp_project(r#"{"web_scripts":["ok.js", 42, "", "   ", null]}"#);
        assert_eq!(read_web_scripts(&dir), vec!["ok.js".to_string()]);
    }

    #[test]
    fn scripts_load_before_the_module_that_boots_the_wasm() {
        // The whole point: a classic script runs to completion before any
        // deferred module, so the library's globals exist before the engine's
        // first frame. Emitting them after the module would mean the engine
        // starts against a global that is not there yet.
        let html = html_template("t", &["https://cdn/colyseus.js".to_string()], "");
        let script_at = html.find("https://cdn/colyseus.js").unwrap();
        let module_at = html.find("<script type=\"module\"").unwrap();
        assert!(script_at < module_at, "extra scripts must precede the module");
        // And after lua-runtime.js, which the Lua VM hook depends on.
        assert!(html.find("lua-runtime.js").unwrap() < script_at);
    }

    #[test]
    fn a_quote_in_a_url_cannot_break_out_of_the_attribute() {
        let html = html_template("t", &[r#"evil.js" onload="alert(1)"#.to_string()], "");
        assert!(
            !html.contains(r#"onload="alert(1)"#),
            "the quote must be escaped, not closed: {html}"
        );
        assert!(html.contains("&quot;"));
    }

    #[test]
    fn a_relative_path_is_left_alone_so_a_project_can_vendor_a_copy() {
        // Depending on a CDN at runtime is a choice, not a requirement.
        let html = html_template("t", &["vendor/colyseus.js".to_string()], "");
        assert!(html.contains(r#"<script src="vendor/colyseus.js"></script>"#));
    }

    #[test]
    fn a_portal_loads_in_the_head_before_the_engine() {
        // YouTube Playables wants its SDK before any game code, and the
        // adapter must wrap `fetch` before the module script starts the wasm.
        let portal = crate::web_platform::head_tags(crate::web_platform::WebPlatform::Poki);
        let html = html_template("t", &[], &portal);
        let sdk = html.find("poki-sdk.js").expect("the SDK is in the page");
        let adapter = html.find("jengine-platform.js").expect("the adapter is in the page");
        let head_end = html.find("</head>").unwrap();
        let engine = html.find("import init").unwrap();
        assert!(sdk < adapter && adapter < head_end && head_end < engine, "{html}");
    }
}
