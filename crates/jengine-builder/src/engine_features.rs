//! Which engine features (`jengine_core::features`) a project's builds
//! carry, and how a build asks Cargo for them (`docs/engine-features-plan.md`).
//!
//! **Chosen from the project itself** by default: a feature is in when the
//! project's assets use it - a component key in a scene or prefab (parts and
//! instance overrides included), a post effect named in a scene or a
//! `post_volume`, a `.gui` node's style (`accessible`), a script calling its
//! namespace (`Fluid.set`) or flat name (`set_fluid_field`), or naming one
//! of its post effects in a string, a data folder only it reads
//! (`assets/dialogue/`), or a `project.jengine` setting that switches it on
//! (`asset_packs`, `liveops`). Each feature found keeps the reasons, which
//! the build log and report show.
//!
//! `settings.engine_features` in `project.jengine` adjusts it:
//!
//! ```json
//! "engine_features": { "include": ["fluid"], "exclude": ["lens_flare"] }
//! "engine_features": { "all": true }
//! ```
//!
//! `include` keeps a feature the scan cannot see (a component made from a
//! string built at run time); `exclude` leaves one out - and **a build fails**
//! when it excludes a feature the project's assets use, naming the files,
//! rather than ship a game whose scene silently lost a component. `all`
//! builds everything, as every build did before features existed. The editor
//! viewport is never narrowed: it has to be able to play anything.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jengine_core::features::{self, Feature};

/// How many reasons a feature keeps (the rest are counted).
const REASONS_KEPT: usize = 3;

/// A project's engine features.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// The features the build carries, in `features::ALL`'s order.
    pub names: Vec<&'static str>,
    /// Why each is in: what the scan found (a few, then a count), or that
    /// `include` / `all` asked for it.
    pub reasons: BTreeMap<&'static str, Vec<String>>,
}

impl Selection {
    /// Every feature - `"all": true`.
    pub fn all() -> Self {
        let names: Vec<&'static str> = features::ALL.iter().map(|f| f.name).collect();
        let reasons = names.iter().map(|n| (*n, vec!["settings.engine_features.all".to_string()])).collect();
        Self { names, reasons }
    }

    pub fn is_all(&self) -> bool {
        self.names.len() == features::ALL.len()
    }

    /// The features left out.
    pub fn left_out(&self) -> Vec<&'static Feature> {
        features::ALL.iter().filter(|f| !self.names.contains(&f.name)).collect()
    }

    /// One line for a build log: what is in and why, what is out.
    pub fn summary(&self) -> String {
        let ins: Vec<String> = self
            .names
            .iter()
            .map(|n| match self.reasons.get(n).and_then(|r| r.first()) {
                Some(why) => format!("{n} ({why})"),
                None => n.to_string(),
            })
            .collect();
        let outs: Vec<&str> = self.left_out().iter().map(|f| f.name).collect();
        format!(
            "Engine features: {}; left out: {}",
            if ins.is_empty() { "none".to_string() } else { ins.join(", ") },
            if outs.is_empty() { "none".to_string() } else { outs.join(", ") }
        )
    }

    /// What a stock host crate (`jengine-runtime`, `jengine-web`) is built
    /// with, beside its other features under `--no-default-features`:
    /// `engine-full`, or each feature - the host's own of that name for one
    /// it hosts too (`Feature::host`), else core's.
    pub fn host_features(&self) -> Vec<String> {
        if self.is_all() {
            vec!["engine-full".to_string()]
        } else {
            self.names
                .iter()
                .map(|n| if hosted(n) { n.to_string() } else { format!("jengine-core/{n}") })
                .collect()
        }
    }

    /// The features a generated crate turns on on the host crate it builds
    /// on (`jengine-runtime`, `jengine-web`), which it depends on without
    /// defaults: the hosted ones among the project's, quoted for a manifest.
    pub fn hosted_features(&self) -> Vec<String> {
        self.names.iter().filter(|n| hosted(n)).map(|n| format!("\"{n}\"")).collect()
    }

    /// The `jengine-core` dependency line a generated crate (a project's
    /// own runtime, Android or web crate) carries, so the engine has
    /// exactly these features: the host crate it builds on is depended on
    /// without its defaults, which would otherwise bring every one.
    pub fn core_dependency(&self, core_path: &str) -> String {
        let list: Vec<String> = self.names.iter().map(|n| format!("\"{n}\"")).collect();
        format!(
            "# The engine features this project builds with (settings.engine_features;\n\
             # docs/engine-features-plan.md).\n\
             jengine-core = {{ path = \"{core_path}\", default-features = false, features = [{}] }}\n",
            list.join(", ")
        )
    }
}

/// What the scan of a project's assets found: each used feature, with why.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Detection {
    pub uses: BTreeMap<&'static str, Vec<String>>,
}

impl Detection {
    fn note(&mut self, feature: &'static str, why: String) {
        let reasons = self.uses.entry(feature).or_default();
        if !reasons.contains(&why) {
            reasons.push(why);
        }
    }

    /// Reasons trimmed to a few and a count, for a message.
    fn told(&self, feature: &str) -> Vec<String> {
        let all = self.uses.get(feature).cloned().unwrap_or_default();
        let mut out: Vec<String> = all.iter().take(REASONS_KEPT).cloned().collect();
        if all.len() > REASONS_KEPT {
            out.push(format!("{} more", all.len() - REASONS_KEPT));
        }
        out
    }
}

/// The engine features `project_root`'s builds carry: what its assets use,
/// adjusted by `settings.engine_features`. Errors on an unknown feature
/// name, and on an `exclude` of a feature the assets use.
pub fn for_project(project_root: &Path) -> anyhow::Result<Selection> {
    let setting = std::fs::read_to_string(project_root.join("project.jengine"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("settings").and_then(|s| s.get("engine_features")).cloned());
    select(&detect(project_root), setting.as_ref())
}

/// [`for_project`] from a scan and the `engine_features` value.
pub fn select(found: &Detection, setting: Option<&serde_json::Value>) -> anyhow::Result<Selection> {
    let list = |key: &str| -> anyhow::Result<Vec<&'static str>> {
        let mut out = Vec::new();
        for name in setting.and_then(|s| s.get(key)).and_then(|v| v.as_array()).into_iter().flatten() {
            let name = name.as_str().unwrap_or_default();
            match features::ALL.iter().find(|f| f.name == name) {
                Some(f) => out.push(f.name),
                None => {
                    let known: Vec<&str> = features::ALL.iter().map(|f| f.name).collect();
                    anyhow::bail!(
                        "settings.engine_features.{key} names `{name}`, which is not an engine feature ({})",
                        known.join(", ")
                    );
                }
            }
        }
        Ok(out)
    };
    let include = list("include")?;
    let exclude = list("exclude")?;

    // Excluding what the assets use is a mistake the build can see.
    let clashes: Vec<String> = exclude
        .iter()
        .filter(|n| found.uses.contains_key(*n))
        .map(|n| format!("`{n}` - {}", found.told(n).join("; ")))
        .collect();
    if !clashes.is_empty() {
        anyhow::bail!(
            "settings.engine_features.exclude leaves out engine features this project uses, so it would \
             build a game that silently loses them: {}. Take them out of `exclude`, or out of the project.",
            clashes.join(" / ")
        );
    }

    if setting.and_then(|s| s.get("all")).and_then(|v| v.as_bool()) == Some(true) {
        let mut all = Selection::all();
        all.names.retain(|n| !exclude.contains(n));
        return Ok(all);
    }
    let mut reasons: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    for f in features::ALL {
        if exclude.contains(&f.name) {
            continue;
        }
        if found.uses.contains_key(f.name) {
            reasons.insert(f.name, found.told(f.name));
        } else if include.contains(&f.name) {
            reasons.insert(f.name, vec!["settings.engine_features.include".to_string()]);
        }
    }
    // What a chosen feature is built on comes with it (Cargo builds it
    // anyway), so the list and the report say what the build carries - and
    // an `exclude` of one is refused, since it could not take effect.
    loop {
        let wanted: Vec<(&'static str, &'static str)> = reasons
            .keys()
            .filter_map(|n| features::ALL.iter().find(|f| f.name == *n))
            .flat_map(|f| f.needs.iter().map(move |need| (f.name, *need)))
            .filter(|(_, need)| !reasons.contains_key(need))
            .collect();
        if wanted.is_empty() {
            break;
        }
        for (by, need) in wanted {
            if exclude.contains(&need) {
                anyhow::bail!(
                    "settings.engine_features.exclude leaves out `{need}`, which `{by}` is built on - \
                     a build carrying `{by}` carries it too"
                );
            }
            let need = features::ALL.iter().find(|f| f.name == need).map(|f| f.name).unwrap_or(need);
            reasons.entry(need).or_default().push(format!("`{by}` is built on it"));
        }
    }
    let names: Vec<&'static str> = features::ALL.iter().map(|f| f.name).filter(|n| reasons.contains_key(n)).collect();
    Ok(Selection { names, reasons })
}

/// Whether the runtimes host feature `name` too (`Feature::host`).
fn hosted(name: &str) -> bool {
    features::ALL.iter().any(|f| f.name == name && f.host)
}

/// Whether a setting is really set: not null, not empty, and - an object or
/// a list - holding something that is (`"asset_packs": { "packs": [] }`
/// declares no pack).
fn is_set(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::String(s) => !s.is_empty(),
        serde_json::Value::Array(a) => a.iter().any(is_set),
        serde_json::Value::Object(m) => m.values().any(is_set),
        serde_json::Value::Number(_) => true,
    }
}

/// Scan `project_root`'s assets for what uses which engine feature.
pub fn detect(project_root: &Path) -> Detection {
    let assets = project_root.join("assets");
    let mut found = Detection::default();
    let settings = std::fs::read_to_string(project_root.join("project.jengine"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("settings").cloned())
        .unwrap_or_default();
    for f in features::ALL {
        for key in f.settings {
            if settings.get(*key).is_some_and(is_set) {
                found.note(f.name, format!("project.jengine sets settings.{key}"));
            }
        }
    }
    let mut files = Vec::new();
    collect(&assets, &mut files);
    for file in &files {
        let rel = file.strip_prefix(project_root).unwrap_or(file).display().to_string().replace('\\', "/");
        let Ok(text) = std::fs::read_to_string(file) else { continue };
        match file.extension().and_then(|e| e.to_str()) {
            Some("scene") | Some("prefab") => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    scan_json(&json, &rel, &mut found);
                }
            }
            Some("lua") | Some("js") => scan_script(&text, &rel, &mut found),
            Some("gui") => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                    scan_gui(&json, &rel, &mut found);
                }
            }
            _ => {}
        }
    }
    for f in features::ALL {
        for dir in f.data_dirs {
            let path = assets.join(dir);
            if std::fs::read_dir(&path).map(|mut d| d.next().is_some()).unwrap_or(false) {
                found.note(f.name, format!("assets/{dir}/ has its data"));
            }
        }
    }
    found
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".jengine" || name == "node_modules" || name == "target" {
            continue;
        }
        if path.is_dir() {
            collect(&path, out);
        } else if matches!(path.extension().and_then(|e| e.to_str()), Some("scene" | "prefab" | "lua" | "js" | "gui")) {
            out.push(path);
        }
    }
}

/// A scene or prefab: every `components` object's keys (and a
/// `prefab_instance`'s override paths), and every `effects` list's names -
/// a scene's `post_process` and a `post_volume` alike.
fn scan_json(value: &serde_json::Value, rel: &str, found: &mut Detection) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::Object(components)) = map.get("components") {
                for key in components.keys() {
                    if let Some(f) = features::of_component(key) {
                        found.note(f.name, format!("{rel} uses the `{key}` component"));
                    }
                }
                let overrides = components.get("prefab_instance").and_then(|p| p.get("overrides")).and_then(|o| o.as_object());
                for path in overrides.into_iter().flat_map(|o| o.keys()) {
                    let key = path.split('.').next().unwrap_or_default();
                    if let Some(f) = features::of_component(key) {
                        found.note(f.name, format!("{rel} overrides the `{key}` component"));
                    }
                }
            }
            for (key, v) in map {
                if let serde_json::Value::String(s) = v {
                    for f in features::ALL {
                        if f.file_types.iter().any(|ext| s.to_ascii_lowercase().ends_with(ext)) {
                            found.note(f.name, format!("{rel} names {s} (`{key}`)"));
                        }
                    }
                }
            }
            if let Some(serde_json::Value::Array(effects)) = map.get("effects") {
                for name in effects.iter().filter_map(|e| e.get("name")).filter_map(|n| n.as_str()) {
                    if let Some(f) = features::of_post_effect(name) {
                        found.note(f.name, format!("{rel} uses the `{name}` post effect"));
                    }
                }
            }
            for v in map.values() {
                scan_json(v, rel, found);
            }
        }
        serde_json::Value::Array(items) => {
            for v in items {
                scan_json(v, rel, found);
            }
        }
        _ => {}
    }
}

/// A `.gui` document: a node's `style` key that is a feature's component
/// (`accessible` - how the node reads to a screen reader).
fn scan_gui(value: &serde_json::Value, rel: &str, found: &mut Detection) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, v) in map {
                if let serde_json::Value::String(s) = v {
                    for f in features::ALL {
                        if f.file_types.iter().any(|ext| s.to_ascii_lowercase().ends_with(ext)) {
                            found.note(f.name, format!("{rel} names {s} (`{key}`)"));
                        }
                    }
                }
            }
            if let Some(serde_json::Value::Object(style)) = map.get("style") {
                for key in style.keys() {
                    if let Some(f) = features::of_component(key) {
                        found.note(f.name, format!("{rel} gives a node `{key}`"));
                    }
                }
            }
            for v in map.values() {
                scan_gui(v, rel, found);
            }
        }
        serde_json::Value::Array(items) => {
            for v in items {
                scan_gui(v, rel, found);
            }
        }
        _ => {}
    }
}

/// A Lua or JavaScript script: a feature's namespace (`Fluid.`), one of its
/// flat function names, or one of its post effects in a string literal
/// (`PostProcess.set_enabled("lens_flare", true)`). A component key in a
/// string is not a use: no script function adds a component by name (a
/// spawn's components come from its prefab, which is scanned), while words
/// like `"smoke"` or `"cloth"` are everywhere in game data.
fn scan_script(text: &str, rel: &str, found: &mut Detection) {
    for f in features::ALL {
        for ns in f.namespaces {
            if has_member_access(text, ns) {
                found.note(f.name, format!("{rel} calls {ns}.*"));
            }
            let flats = jengine_core::scripting::namespaces::CORE.iter().filter(|(n, _, _)| n == ns);
            for (_, _, flat) in flats {
                if has_word(text, flat) {
                    found.note(f.name, format!("{rel} calls {flat}"));
                }
            }
        }
        for key in f.post_effects {
            if text.contains(&format!("\"{key}\"")) || text.contains(&format!("'{key}'")) {
                found.note(f.name, format!("{rel} names \"{key}\""));
            }
        }
        for call in f.calls {
            let Some((ns, func)) = call.split_once('.') else { continue };
            if has_word(text, call) {
                found.note(f.name, format!("{rel} calls {call}"));
            }
            let flat = jengine_core::scripting::namespaces::CORE.iter().find(|(n, m, _)| *n == ns && *m == func);
            if let Some((_, _, flat)) = flat
                && has_word(text, flat)
            {
                found.note(f.name, format!("{rel} calls {flat}"));
            }
        }
        for ext in f.file_types {
            if names_file(text, ext) {
                found.note(f.name, format!("{rel} names a {ext} file"));
            }
        }
    }
}

/// A string in `text` ending in `ext` (`.ttf`), any case: the extension
/// right before a closing quote.
fn names_file(text: &str, ext: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.match_indices(ext).any(|(i, _)| matches!(lower[i + ext.len()..].chars().next(), Some('"' | '\'')))
}

fn is_ident(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `word` as a whole identifier somewhere in `text`.
fn has_word(text: &str, word: &str) -> bool {
    text.match_indices(word).any(|(i, _)| {
        let before = text[..i].chars().next_back().is_none_or(|c| !is_ident(c));
        let after = text[i + word.len()..].chars().next().is_none_or(|c| !is_ident(c));
        before && after
    })
}

/// `ns.` with `ns` a whole identifier - `Fluid.set`, not `MyFluid.set`.
fn has_member_access(text: &str, ns: &str) -> bool {
    let pattern = format!("{ns}.");
    text.match_indices(&pattern).any(|(i, _)| text[..i].chars().next_back().is_none_or(|c| !is_ident(c)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn uses(pairs: &[(&'static str, &str)]) -> Detection {
        let mut d = Detection::default();
        for (f, why) in pairs {
            d.note(f, why.to_string());
        }
        d
    }

    #[test]
    fn a_project_gets_what_it_uses_and_nothing_else() {
        let s = select(&uses(&[("dialogue", "x.lua calls Dialogue.*")]), None).unwrap();
        assert_eq!(s.names, vec!["dialogue"]);
        assert_eq!(s.reasons["dialogue"], vec!["x.lua calls Dialogue.*".to_string()]);
        assert_eq!(s.host_features(), vec!["jengine-core/dialogue".to_string()]);
        assert!(select(&Detection::default(), None).unwrap().names.is_empty(), "an empty project needs nothing");
    }

    #[test]
    fn include_all_and_exclude_adjust_it() {
        let s = select(&Detection::default(), Some(&json!({ "include": ["fluid"] }))).unwrap();
        assert_eq!(s.names, vec!["fluid"]);
        assert!(select(&Detection::default(), Some(&json!({ "all": true }))).unwrap().is_all());
        let s = select(&Detection::default(), Some(&json!({ "all": true, "exclude": ["lens_flare"] }))).unwrap();
        assert!(!s.names.contains(&"lens_flare") && s.names.contains(&"fluid"));
    }

    #[test]
    fn excluding_what_the_project_uses_fails_and_says_where() {
        let found = uses(&[("fluid", "assets/scenes/a.scene uses the `fluid` component")]);
        let e = select(&found, Some(&json!({ "exclude": ["fluid"] }))).unwrap_err().to_string();
        assert!(e.contains("`fluid`") && e.contains("a.scene"), "{e}");
    }

    #[test]
    fn a_misspelt_feature_is_refused() {
        for key in ["include", "exclude"] {
            let e = select(&Detection::default(), Some(&json!({ key: ["fluids"] }))).unwrap_err().to_string();
            assert!(e.contains("fluids") && e.contains(key), "{e}");
        }
    }

    #[test]
    fn every_kind_of_use_is_seen() {
        let mut d = Detection::default();
        scan_json(
            &json!({ "entities": [
                { "name": "Pool", "components": { "fluid": {} } },
                { "name": "Cam", "components": {}, "children": [{ "name": "In", "components": { "dialogue": {} } }] }
            ], "post_process": { "effects": [{ "name": "lens_flare" }] } }),
            "s.scene",
            &mut d,
        );
        assert!(d.uses.contains_key("fluid") && d.uses.contains_key("dialogue") && d.uses.contains_key("lens_flare"), "{d:?}");

        let mut d = Detection::default();
        scan_json(&json!({ "components": { "post_volume": { "effects": [{ "name": "lens_flare" }] } } }), "p.prefab", &mut d);
        scan_json(&json!({ "components": { "prefab_instance": { "prefab_id": "x", "overrides": { "fluid.viscosity": 2 } } } }), "q.scene", &mut d);
        assert!(d.uses.contains_key("lens_flare") && d.uses.contains_key("fluid"), "{d:?}");

        let mut d = Detection::default();
        scan_script("Dialogue.start(e, 'hi')", "a.lua", &mut d);
        scan_script("set_fluid_field(e, 'pour_rate', 3);", "b.js", &mut d);
        scan_script("PostProcess.enable('lens_flare')", "c.lua", &mut d);
        assert!(d.uses.contains_key("dialogue") && d.uses.contains_key("fluid") && d.uses.contains_key("lens_flare"), "{d:?}");

        let mut d = Detection::default();
        scan_gui(&json!({ "root": { "children": [{ "style": { "accessible": { "label": "Play" } } }] } }), "m.gui", &mut d);
        assert!(d.uses.contains_key("accessibility"), "{d:?}");
        assert!(is_set(&json!({ "packs": [{ "name": "music" }] })) && !is_set(&json!({ "packs": [] })));

        let mut d = Detection::default();
        scan_json(&json!({ "components": { "text": { "font_id": "fonts/Noto.TTF" } } }), "t.scene", &mut d);
        scan_script("PostProcess.set_lut('luts/dusk.png', 1)", "l.lua", &mut d);
        scan_script("set_sprite_label(e, 'hat', 'crown')", "s.js", &mut d);
        assert!(
            d.uses.contains_key("dynamic_font") && d.uses.contains_key("lut") && d.uses.contains_key("sprite_library"),
            "{d:?}"
        );

        let mut d = Detection::default();
        scan_script("MyFluid.set(x) -- fluidity", "d.lua", &mut d);
        scan_script("local items = { { id = \"cloth\" }, { art = 'jelly' } }", "e.lua", &mut d);
        assert!(d.uses.is_empty(), "near-misses are not uses: {d:?}");
    }

    #[test]
    fn what_a_feature_is_built_on_comes_with_it() {
        let mut d = Detection::default();
        d.note("trajectory", "golf.scene uses the `trajectory` component".into());
        let s = select(&d, None).unwrap();
        assert_eq!(s.names, vec!["trajectory", "line2d"]);
        assert_eq!(s.reasons["line2d"], vec!["`trajectory` is built on it".to_string()]);
        let err = select(&d, Some(&json!({ "exclude": ["line2d"] }))).unwrap_err().to_string();
        assert!(err.contains("`trajectory` is built on"), "{err}");
    }

    #[test]
    fn a_hosted_feature_is_asked_of_the_runtime() {
        let s = select(&Detection::default(), Some(&json!({ "include": ["video", "fluid"] }))).unwrap();
        // The runtime's own `video` (core's and its decoder), core's `fluid`.
        assert_eq!(s.host_features(), vec!["jengine-core/fluid".to_string(), "video".to_string()]);
        assert_eq!(s.hosted_features(), vec!["\"video\"".to_string()]);
    }

    #[test]
    fn every_feature_can_be_found() {
        for f in features::ALL {
            assert!(
                !(f.components.is_empty()
                    && f.namespaces.is_empty()
                    && f.post_effects.is_empty()
                    && f.data_dirs.is_empty()
                    && f.settings.is_empty()
                    && f.calls.is_empty()
                    && f.file_types.is_empty()),
                "`{}` has nothing a scan could find it by - it would never be built in",
                f.name
            );
        }
    }
}
