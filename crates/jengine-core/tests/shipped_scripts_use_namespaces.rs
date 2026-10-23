//! Every shipped game script calls the namespaced API, and every namespaced
//! call it makes exists.
//!
//! The games were migrated to `Sprite.set_color` and friends in Sep 2026
//! (about 7,400 call sites). The flat names still work - Rust test snippets
//! and JavaScript scripts use them - but a game script written today should
//! read like the docs, and this keeps it that way. It also checks the other
//! direction, which is the one that breaks things: a call to a namespaced
//! name no runtime installs is `attempt to call a nil value` at play time,
//! and a script compiling cleanly says nothing about it.
//!
//! Package namespaces are derived here exactly as the runtimes derive them -
//! `scripting::namespaces::package_entries` - from the commands the packages
//! register, read out of their source the way
//! `projects_link_the_packages_their_scenes_use` reads their components.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use jengine_core::package::script_commands::PackageCommandInfo;
use jengine_core::scripting::namespaces::{all_entries, DUPLICATES};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn files(dir: &Path, exts: &[&str], out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            files(&path, exts, out);
        } else if path.extension().is_some_and(|e| exts.iter().any(|x| e == *x)) {
            out.push(path);
        }
    }
}

/// `(package, command)` for every `.register(PACKAGE, "name", ...)` under
/// `packages/`. The name may sit on the line after the call.
fn package_commands() -> Vec<PackageCommandInfo> {
    let mut out = Vec::new();
    let mut crates: Vec<PathBuf> = std::fs::read_dir(repo().join("packages"))
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("src/lib.rs").is_file())
        .collect();
    crates.sort();
    for krate in crates {
        let lib = std::fs::read_to_string(krate.join("src/lib.rs")).unwrap();
        let package_const = lib
            .split("pub const PACKAGE: &str = \"")
            .nth(1)
            .and_then(|r| r.split('"').next())
            .map(str::to_string);
        let mut sources = Vec::new();
        files(&krate.join("src"), &["rs"], &mut sources);
        sources.sort();
        for src in sources {
            let text = std::fs::read_to_string(&src).unwrap();
            // A local macro can do the registering - `simple!("gb_enter", ...)`
            // in one package - and its invocations name the commands. A
            // scan that only read `.register(` calls missed ten of its commands.
            let mut macros = Vec::new();
            let mut m = text.as_str();
            while let Some(i) = m.find("macro_rules!") {
                m = &m[i + "macro_rules!".len()..];
                let name: String = m.trim_start().chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
                let body_end = m.find("\n}").unwrap_or(m.len());
                if m[..body_end].contains(".register(") && !name.is_empty() {
                    macros.push(name);
                }
            }
            for mac in &macros {
                let call = format!("{mac}!(");
                let mut r = text.as_str();
                while let Some(i) = r.find(&call) {
                    r = &r[i + call.len()..];
                    let Some(after) = r.trim_start().strip_prefix('"') else { continue };
                    let Some(end) = after.find('"') else { continue };
                    let name = &after[..end];
                    if let Some(package) = package_const.clone()
                        && !out.iter().any(|i: &PackageCommandInfo| i.name == name)
                    {
                        out.push(PackageCommandInfo {
                            name: name.to_string(),
                            tag: format!("pkg/{package}/{name}"),
                            package,
                            args: Vec::new(),
                        });
                    }
                }
            }
            let mut rest = text.as_str();
            while let Some(i) = rest.find(".register(") {
                rest = &rest[i + ".register(".len()..];
                let args = rest.trim_start();
                // `crate::PACKAGE` / `super::PACKAGE` from a submodule names
                // the same constant; read as no package, every command a
                // submodule registered looked uninstalled (Dual Spark).
                let args = args.strip_prefix("crate::").or_else(|| args.strip_prefix("super::")).unwrap_or(args);
                let (package, after) = if let Some(a) = args.strip_prefix("PACKAGE") {
                    (package_const.clone(), a)
                } else if let Some(a) = args.strip_prefix('"') {
                    let end = a.find('"').unwrap_or(0);
                    (Some(a[..end].to_string()), &a[end + 1..])
                } else {
                    continue;
                };
                let Some(after) = after.trim_start().strip_prefix(',') else { continue };
                let Some(after) = after.trim_start().strip_prefix('"') else { continue };
                let Some(end) = after.find('"') else { continue };
                let name = &after[..end];
                if let Some(package) = package {
                    if !out.iter().any(|i: &PackageCommandInfo| i.name == name) {
                        out.push(PackageCommandInfo {
                            name: name.to_string(),
                            tag: format!("pkg/{package}/{name}"),
                            package,
                            args: Vec::new(),
                        });
                    }
                }
            }
        }
    }
    out
}

/// The code of a Lua file with strings and comments blanked out (replaced by
/// spaces, so offsets and line numbers survive).
fn code_only(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = src.as_bytes().to_vec();
    let mut i = 0;
    let long_close = |i: usize| -> Option<(usize, String)> {
        // `[[` or `[==[` at i: returns (len of opener, closer)
        let mut j = i + 1;
        while j < b.len() && b[j] == b'=' {
            j += 1;
        }
        (j < b.len() && b[j] == b'[').then(|| (j + 1 - i, format!("]{}]", "=".repeat(j - i - 1))))
    };
    while i < b.len() {
        let start = i;
        let end = if b[i..].starts_with(b"--") {
            if let Some((open, close)) = (i + 2 < b.len() && b[i + 2] == b'[').then(|| long_close(i + 2)).flatten() {
                src[i + 2 + open..].find(&close).map_or(b.len(), |k| i + 2 + open + k + close.len())
            } else {
                src[i..].find('\n').map_or(b.len(), |k| i + k)
            }
        } else if b[i] == b'[' && long_close(i).is_some() {
            let (open, close) = long_close(i).unwrap();
            src[i + open..].find(&close).map_or(b.len(), |k| i + open + k + close.len())
        } else if b[i] == b'"' || b[i] == b'\'' {
            let q = b[i];
            let mut j = i + 1;
            while j < b.len() && b[j] != q && b[j] != b'\n' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            (j + 1).min(b.len())
        } else {
            i += 1;
            continue;
        };
        for c in &mut out[start..end] {
            if *c != b'\n' {
                *c = b' ';
            }
        }
        i = end;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn is_ident(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Identifiers with their byte offsets.
fn idents(code: &str) -> Vec<(usize, &str)> {
    let b = code.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if (b[i].is_ascii_alphabetic() || b[i] == b'_') && (i == 0 || !is_ident(b[i - 1])) {
            let s = i;
            while i < b.len() && is_ident(b[i]) {
                i += 1;
            }
            out.push((s, &code[s..i]));
        } else {
            i += 1;
        }
    }
    out
}

/// Names the file binds itself - locals, parameters, loop variables and
/// function definitions - which a flat call may legitimately refer to (five
/// scripts have their own `clamp`).
fn bound(code: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let ids = idents(code);
    for (k, (_, word)) in ids.iter().enumerate() {
        match *word {
            "local" | "for" => {
                // `local a, b` / `local function f` / `for k, v`
                let mut j = k + 1;
                if ids.get(j).is_some_and(|(_, w)| *w == "function") {
                    j += 1;
                }
                while let Some((pos, name)) = ids.get(j) {
                    out.insert(name.to_string());
                    let tail = code[pos + name.len()..].trim_start();
                    if !tail.starts_with(',') {
                        break;
                    }
                    j += 1;
                }
            }
            "function" => {
                let rest = &code[ids[k].0 + 8..];
                let open = rest.find('(').unwrap_or(0);
                let name = rest[..open].trim();
                if !name.is_empty() && !name.contains(['.', ':']) {
                    out.insert(name.to_string());
                }
                let close = rest[open..].find(')').map_or(0, |c| open + c);
                for p in rest[open + 1..close].split(',') {
                    out.insert(p.trim().to_string());
                }
            }
            _ => {}
        }
    }
    out
}

#[test]
fn every_shipped_script_uses_namespaced_names() {
    let entries = all_entries(&package_commands());
    let flat: BTreeSet<&str> = entries
        .iter()
        .map(|e| e.flat.as_str())
        .chain(DUPLICATES.iter().map(|(f, _)| *f))
        .collect();
    let namespaces: BTreeSet<&str> = entries.iter().map(|e| e.namespace.as_str()).collect();
    let qualified: BTreeSet<String> = entries.iter().map(|e| e.qualified()).collect();

    let mut scripts = Vec::new();
    files(&repo().join("projects"), &["lua", "luau"], &mut scripts);
    files(&repo().join("templates"), &["lua", "luau"], &mut scripts);
    scripts.sort();
    assert!(scripts.len() > 150, "found only {} scripts", scripts.len());

    let mut flat_calls = Vec::new();
    let mut unknown = Vec::new();
    for path in &scripts {
        let src = std::fs::read_to_string(path).unwrap();
        let code = code_only(&src);
        let own = bound(&code);
        let rel = path.strip_prefix(repo()).unwrap_or(path).display().to_string();
        let line_of = |pos: usize| code[..pos].matches('\n').count() + 1;
        for (pos, name) in idents(&code) {
            let before = code[..pos].trim_end();
            let after = code[pos + name.len()..].trim_start();
            let member = before.ends_with('.') || before.ends_with(':');
            if !member && flat.contains(name) && !own.contains(name) && after.starts_with('(')
                && !before.ends_with("function")
            {
                flat_calls.push(format!("{rel}:{}: {name}(", line_of(pos)));
            }
            // `Ns.short(` - a namespace, then a field call.
            if !member && namespaces.contains(name)
                && let Some(field) = after.strip_prefix('.')
            {
                let short: String = field.bytes().take_while(|c| is_ident(*c)).map(char::from).collect();
                if field[short.len()..].trim_start().starts_with('(') && !qualified.contains(&format!("{name}.{short}")) {
                    unknown.push(format!("{rel}:{}: {name}.{short}", line_of(pos)));
                }
            }
            if name == "math" && !member {
                for helper in ["lerp", "clamp", "distance", "angle_between"] {
                    if after.starts_with(&format!(".{helper}(")) {
                        unknown.push(format!("{rel}:{}: math.{helper} is Math.{helper}", line_of(pos)));
                    }
                }
            }
        }
    }
    assert!(
        unknown.is_empty(),
        "calls to namespaced names no runtime installs ({}):\n{}",
        unknown.len(),
        unknown.join("\n")
    );
    assert!(
        flat_calls.is_empty(),
        "flat script-API calls in shipped scripts - use the namespaced name \
         (`scripting::namespaces`) ({}):\n{}",
        flat_calls.len(),
        flat_calls.iter().take(40).cloned().collect::<Vec<_>>().join("\n")
    );
}

/// No script binds a namespace's own name.
///
/// Fourteen did, the same way: `local GUI = "ui/hub.gui"`, the document
/// path every `gui_*` call took. The namespace migration turned
/// `gui_set(GUI, ...)` into `GUI.set(GUI, ...)`, which then indexed that
/// string - `attempt to call a nil value` on the first GUI call, so the
/// arcade hub, most of one game's menus and five test-lab scenes died
/// in `init`, in every runtime. The test above could not see it: the name is
/// a namespace and the call exists; it is the local that is wrong.
#[test]
fn no_shipped_script_shadows_a_namespace() {
    let entries = all_entries(&package_commands());
    let namespaces: BTreeSet<&str> = entries.iter().map(|e| e.namespace.as_str()).collect();

    let mut scripts = Vec::new();
    files(&repo().join("projects"), &["lua", "luau"], &mut scripts);
    files(&repo().join("templates"), &["lua", "luau"], &mut scripts);
    scripts.sort();

    let mut shadowed = Vec::new();
    for path in &scripts {
        let code = code_only(&std::fs::read_to_string(path).unwrap());
        let rel = path.strip_prefix(repo()).unwrap_or(path).display().to_string();
        for name in bound(&code) {
            if namespaces.contains(name.as_str()) {
                shadowed.push(format!("{rel}: binds `{name}`"));
            }
        }
    }
    assert!(
        shadowed.is_empty(),
        "scripts that name a local, parameter or function after a script \
         namespace, hiding it for the rest of the file ({}):\n{}",
        shadowed.len(),
        shadowed.join("\n")
    );
}
