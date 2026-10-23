//! Where the engine's source is.
//!
//! A build compiles the runtime from source - a project's own Rust packages
//! are linked into a crate generated beside it, with path dependencies on the
//! engine's crates - so everything that builds needs an engine checkout. Both
//! the `jengine` CLI and the editor ask here, in this order:
//!
//! 1. **`JENGINE_HOME`**, when set: the checkout to use, wherever the tool is.
//!    An installed editor or a `cargo install`ed CLI is found this way.
//! 2. **The current directory and its parents**: running `jengine` from
//!    inside a checkout, or a project inside one, uses that checkout.
//! 3. **The executable's directory and its parents**: a tool built inside a
//!    checkout (`target/debug/jengine.exe`, the editor in `cargo tauri dev`).
//! 4. **The copy an installed editor ships**: the engine's source zipped
//!    into its resources ([`pack`]), unpacked on first use into a writable
//!    folder the editor names ([`use_bundle`]). After (1) - (3), so a user
//!    working in a checkout builds against it; before (5), which for an
//!    installed editor is the machine it was built on.
//! 5. **Where this crate was compiled**, if that checkout still exists - a
//!    `cargo install --path` from a checkout keeps working while it does.
//!
//! The CLI used to take only (4) and the editor only (3), so an installed
//! editor failed every build with "Could not find engine workspace root".

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// The environment variable that names an engine checkout.
pub const ENV: &str = "JENGINE_HOME";

/// Whether `dir` is an engine checkout: a Cargo workspace holding the core
/// crate and the runtime a build compiles.
pub fn is_engine_root(dir: &Path) -> bool {
    let manifest = dir.join("Cargo.toml");
    manifest.is_file()
        && dir.join("crates").join("jengine-core").join("Cargo.toml").is_file()
        && dir.join("crates").join("jengine-runtime").join("Cargo.toml").is_file()
        && std::fs::read_to_string(&manifest).is_ok_and(|c| c.contains("[workspace]"))
}

/// The nearest engine checkout at or above `start`.
pub fn find_above(start: &Path) -> Option<PathBuf> {
    start.ancestors().find(|d| is_engine_root(d)).map(Path::to_path_buf)
}

/// Locate the engine checkout - see the module docs for the order.
pub fn locate() -> Result<PathBuf, String> {
    locate_with(
        std::env::var_os(ENV).map(PathBuf::from),
        std::env::current_dir().ok(),
        std::env::current_exe().ok(),
    )
}

/// [`locate`] with its inputs given, so it can be tested.
pub fn locate_with(
    home: Option<PathBuf>,
    cwd: Option<PathBuf>,
    exe: Option<PathBuf>,
) -> Result<PathBuf, String> {
    if let Some(home) = home.filter(|h| !h.as_os_str().is_empty()) {
        // An explicit choice that is wrong is an error, not a reason to guess.
        return if is_engine_root(&home) {
            Ok(canonical(home))
        } else {
            Err(format!(
                "{ENV} is set to {}, which is not a jEngine checkout (no Cargo workspace \
                 with crates/jengine-core and crates/jengine-runtime there)",
                home.display()
            ))
        };
    }
    let compiled = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    cwd.as_deref()
        .and_then(find_above)
        .or_else(|| exe.as_deref().and_then(|e| e.parent()).and_then(find_above))
        .map(canonical)
        .or_else(|| {
            let bundle = BUNDLE.get()?;
            match bundle.unpacked() {
                Ok(root) => Some(root),
                Err(e) => {
                    log::error!("Unpacking the engine source the editor ships: {e:#}");
                    None
                }
            }
        })
        .or_else(|| is_engine_root(&compiled).then(|| canonical(compiled)))
        .ok_or_else(|| {
            format!(
                "could not find the jEngine source. Building a game compiles the engine, so it \
                 needs a checkout (git clone https://github.com/jkilpala/jengine) - set {ENV} \
                 to its folder, or run from inside it"
            )
        })
}

// ---------------------------------------------------------------- the bundle

/// What a bundle holds, from an engine tree's root: the files and folders a
/// build compiles from. The editor, the website and the example projects stay
/// out - a build never reads them - so the workspace's members are cut to
/// what is there ([`bundled_manifest`]).
const BUNDLED_FILES: [&str; 6] =
    ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml", "LICENSE", "THIRD-PARTY-NOTICES.md"];
const BUNDLED_DIRS: [&str; 5] = ["crates", "packages", "vendor", "templates", "assets"];
/// Folders never bundled, wherever they are: build output and local state.
const SKIPPED_DIRS: [&str; 4] = ["target", "node_modules", ".jengine", ".git"];
/// Written last into an unpacked bundle, naming the archive it came from.
const UNPACKED_MARK: &str = ".jengine-unpacked";

/// The engine source an installed editor ships, and where to unpack it.
#[derive(Debug, Clone)]
pub struct Bundle {
    /// The zip [`pack`] made, in the editor's resources (read-only).
    pub archive: PathBuf,
    /// A writable folder to unpack it into; build output goes under it too.
    pub into: PathBuf,
}

static BUNDLE: std::sync::OnceLock<Bundle> = std::sync::OnceLock::new();

/// Make `bundle` the last place [`locate`] looks - what an installed editor
/// calls once at start.
pub fn use_bundle(bundle: Bundle) {
    let _ = BUNDLE.set(bundle);
}

impl Bundle {
    /// The unpacked engine, unpacking it first if it is not there yet or came
    /// from another archive (the editor was updated). `target/` survives a
    /// re-unpack, so an update does not throw the build cache away.
    pub fn unpacked(&self) -> Result<PathBuf> {
        let meta = std::fs::metadata(&self.archive)
            .with_context(|| format!("no engine source at {}", self.archive.display()))?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        let mark = format!("{} {} {modified}", crate::packages::display_path(&self.archive), meta.len());
        let root = canonical(self.into.clone());
        if std::fs::read_to_string(root.join(UNPACKED_MARK)).is_ok_and(|m| m == mark) && is_engine_root(&root) {
            return Ok(root);
        }
        crate::progress::step(&format!("Unpacking the engine source into {}...", root.display()));
        if root.is_dir() {
            for entry in std::fs::read_dir(&root)? {
                let path = entry?.path();
                if path.file_name().is_some_and(|n| n == "target") {
                    continue;
                }
                if path.is_dir() { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) }
                    .with_context(|| format!("clearing {}", path.display()))?;
            }
        }
        let count = unpack(&self.archive, &root)?;
        std::fs::write(root.join(UNPACKED_MARK), mark)?;
        if !is_engine_root(&root) {
            anyhow::bail!("{} did not unpack into an engine tree", self.archive.display());
        }
        crate::progress::step(&format!("Unpacked {count} engine source files."));
        Ok(root)
    }
}

/// Zip the engine source under `from` into `out` - what an installer ships.
/// Returns the number of files.
pub fn pack(from: &Path, out: &Path) -> Result<usize> {
    if !is_engine_root(from) {
        anyhow::bail!("{} is not an engine tree", from.display());
    }
    let mut files: Vec<PathBuf> = Vec::new();
    for dir in BUNDLED_DIRS {
        collect(&from.join(dir), &mut files)?;
    }
    for file in BUNDLED_FILES {
        if from.join(file).is_file() {
            files.push(from.join(file));
        }
    }
    files.sort();
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut zip = zip::ZipWriter::new(
        std::fs::File::create(out).with_context(|| format!("creating {}", out.display()))?,
    );
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for file in &files {
        let name = file.strip_prefix(from).expect("collected under from").to_string_lossy().replace('\\', "/");
        let bytes = if name == "Cargo.toml" {
            bundled_manifest(&std::fs::read_to_string(file)?, from)?.into_bytes()
        } else {
            std::fs::read(file).with_context(|| format!("reading {}", file.display()))?
        };
        zip.start_file(name, options)?;
        zip.write_all(&bytes)?;
    }
    zip.finish()?;
    Ok(files.len())
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let path = entry?.path();
        if path.is_dir() {
            if !path.file_name().is_some_and(|n| SKIPPED_DIRS.iter().any(|s| n == *s)) {
                collect(&path, out)?;
            }
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// The workspace manifest with its members cut to those a bundle carries -
/// cargo refuses a workspace naming a member that is not there.
fn bundled_manifest(text: &str, from: &Path) -> Result<String> {
    let mut doc: toml::Table = text.parse().context("parsing the workspace Cargo.toml")?;
    let members = doc
        .get_mut("workspace")
        .and_then(|w| w.get_mut("members"))
        .and_then(|m| m.as_array_mut())
        .context("the workspace Cargo.toml has no members")?;
    members.retain(|m| {
        let Some(m) = m.as_str() else { return false };
        let bundled = BUNDLED_DIRS.iter().any(|d| m.split('/').next() == Some(*d));
        bundled && !m.contains('*') && from.join(m).join("Cargo.toml").is_file()
    });
    Ok(toml::to_string(&doc)?)
}

/// Unpack a zip [`pack`] made into `into`. Returns the number of files.
fn unpack(archive: &Path, into: &Path) -> Result<usize> {
    let file = std::fs::File::open(archive).with_context(|| format!("opening {}", archive.display()))?;
    let mut zip = zip::ZipArchive::new(file).with_context(|| format!("reading {}", archive.display()))?;
    let mut count = 0;
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        // `enclosed_name` refuses `..` and absolute names.
        let Some(name) = entry.enclosed_name() else { continue };
        let dest = into.join(name);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
        std::fs::write(&dest, bytes).with_context(|| format!("writing {}", dest.display()))?;
        count += 1;
    }
    Ok(count)
}

fn canonical(p: PathBuf) -> PathBuf {
    // `canonicalize` on Windows gives a `\\?\` path, which cargo and the
    // generated crates' relative paths do not want.
    std::path::absolute(&p).unwrap_or(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_engine(root: &Path) {
        for c in ["jengine-core", "jengine-runtime"] {
            std::fs::create_dir_all(root.join("crates").join(c)).unwrap();
            std::fs::write(root.join("crates").join(c).join("Cargo.toml"), "[package]\n").unwrap();
        }
        std::fs::write(root.join("Cargo.toml"), "[workspace]\nmembers = []\n").unwrap();
    }

    #[test]
    fn jengine_home_wins_and_a_wrong_one_is_an_error() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        fake_engine(a.path());
        fake_engine(b.path());
        let found = locate_with(Some(a.path().into()), Some(b.path().into()), None).unwrap();
        assert!(is_engine_root(&found) && found.ends_with(a.path().file_name().unwrap()));
        let empty = tempfile::tempdir().unwrap();
        let err = locate_with(Some(empty.path().into()), Some(b.path().into()), None).unwrap_err();
        assert!(err.contains(ENV), "{err}");
    }

    #[test]
    fn the_current_directory_is_searched_upwards_before_the_executable() {
        let cwd_engine = tempfile::tempdir().unwrap();
        let exe_engine = tempfile::tempdir().unwrap();
        fake_engine(cwd_engine.path());
        fake_engine(exe_engine.path());
        let deep = cwd_engine.path().join("projects").join("game");
        std::fs::create_dir_all(&deep).unwrap();
        let exe = exe_engine.path().join("target").join("debug").join("jengine.exe");
        let found = locate_with(None, Some(deep), Some(exe.clone())).unwrap();
        assert!(found.ends_with(cwd_engine.path().file_name().unwrap()));
        let outside = tempfile::tempdir().unwrap();
        let found = locate_with(None, Some(outside.path().into()), Some(exe)).unwrap();
        assert!(found.ends_with(exe_engine.path().file_name().unwrap()));
    }

    #[test]
    fn a_workspace_that_is_not_the_engine_is_passed_over() {
        let game = tempfile::tempdir().unwrap();
        std::fs::write(game.path().join("Cargo.toml"), "[workspace]\n").unwrap();
        assert!(!is_engine_root(game.path()));
    }

    #[test]
    fn a_bundle_unpacks_into_an_engine_whose_workspace_names_only_what_it_carries() {
        let src = tempfile::tempdir().unwrap();
        fake_engine(src.path());
        std::fs::write(
            src.path().join("Cargo.toml"),
            r#"[workspace]
members = ["crates/jengine-core", "crates/jengine-runtime", "editor/src-tauri", "projects/game/server", "crates/*"]

[patch.crates-io]
winit = { path = "vendor/winit" }
"#,
        )
        .unwrap();
        std::fs::create_dir_all(src.path().join("crates/jengine-core/target/debug")).unwrap();
        std::fs::write(src.path().join("crates/jengine-core/target/debug/big.rlib"), "x").unwrap();
        std::fs::create_dir_all(src.path().join("editor/src")).unwrap();
        std::fs::write(src.path().join("editor/src/App.svelte"), "x").unwrap();

        let out = tempfile::tempdir().unwrap();
        let archive = out.path().join("engine.zip");
        assert_eq!(pack(src.path(), &archive).unwrap(), 3, "two crate manifests and the workspace's");
        let bundle = Bundle { archive, into: out.path().join("engine") };
        let root = bundle.unpacked().unwrap();
        assert!(is_engine_root(&root));
        assert!(!root.join("editor").exists() && !root.join("crates/jengine-core/target").exists());
        let manifest: toml::Table = std::fs::read_to_string(root.join("Cargo.toml")).unwrap().parse().unwrap();
        let members: Vec<&str> =
            manifest["workspace"]["members"].as_array().unwrap().iter().map(|m| m.as_str().unwrap()).collect();
        assert_eq!(members, ["crates/jengine-core", "crates/jengine-runtime"]);
        assert!(manifest["patch"]["crates-io"]["winit"].is_table(), "the rest of the manifest is kept");

        // A second call reuses it; build output in target/ survives a re-unpack.
        std::fs::create_dir_all(root.join("target")).unwrap();
        std::fs::write(root.join("target/kept"), "x").unwrap();
        std::fs::write(root.join("stray"), "x").unwrap();
        assert_eq!(bundle.unpacked().unwrap(), root);
        assert!(root.join("stray").exists(), "the same archive is not unpacked again");
        std::fs::write(root.join(UNPACKED_MARK), "another archive").unwrap();
        bundle.unpacked().unwrap();
        assert!(!root.join("stray").exists() && root.join("target/kept").exists());
    }

    #[test]
    fn this_checkout_is_found() {
        let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        assert!(locate_with(None, Some(here), None).is_ok_and(|p| is_engine_root(&p)));
    }
}
