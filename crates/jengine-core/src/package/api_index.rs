//! Generates the API Index table in `docs/scripting-api.md`.
//!
//! The index was hand-maintained and fell 50 functions behind, because the
//! documented procedure said to grep `globals.set(…)` out of `luau.rs` — which
//! misses everything registered through [`crate::package::core_script_api`]'s
//! `reg!` macro and installed dynamically. That single markdown file is
//! upstream of the documentation site *and* the machine-readable AI spec, so
//! the gap propagated into what tools are told the engine can do.
//!
//! This renders the table from the registration sites instead:
//!
//! * [`ScriptApiRegistry`] — name, description, argument names and types,
//!   return type. The good source: real data, no parsing.
//! * `adapters/luau.rs` — globals the Luau adapter sets directly, which never
//!   pass through the registry. Only their names are recoverable, so their
//!   signatures are carried over from the existing table.
//! * `BRIDGE_FUNCTIONS` in `editor/src/lib/lua-runtime.ts` — which functions
//!   the editor and web builds actually expose, for the Runtimes column.
//!
//! The caller supplies those sources as strings; nothing here touches the
//! filesystem, so the example that writes the doc and the test that checks it
//! stay honest about reading the same inputs.

use std::collections::{BTreeMap, BTreeSet};

use super::script_api::{ScriptApiRegistry, ScriptValueType};

/// Everything needed to render the table.
pub struct IndexInputs<'a> {
    /// Populated with `core_script_api::register_core_script_api`.
    pub registry: &'a ScriptApiRegistry,
    /// Source of `scripting/bridge/adapters/luau.rs`.
    pub luau_rs: &'a str,
    /// Source of `editor/src/lib/lua-runtime.ts`.
    pub lua_runtime_ts: &'a str,
    /// Current `docs/scripting-api.md`, for categories and signatures that
    /// only exist there.
    pub existing_doc: &'a str,
}

/// One rendered row, keyed by the function's flat global name.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    /// Only used for a function no namespace table knows, which
    /// `script_namespaces_reach_every_runtime` keeps from happening natively.
    category: String,
    /// The argument list, without parentheses.
    args: String,
    returns: String,
    side_effect: String,
    runtimes: String,
    name: String,
}

/// How a row is presented: `(namespace column, call, flat-name column)`.
fn presentation(row: &Row) -> (String, String, String) {
    use crate::scripting::namespaces::{CORE, DUPLICATES, GLOBALS};
    if let Some((ns, short, _)) = CORE.iter().find(|(_, _, flat)| *flat == row.name) {
        return (ns.to_string(), format!("`{ns}.{short}({})`", row.args), format!("`{}`", row.name));
    }
    if GLOBALS.contains(&row.name.as_str()) {
        return ("Global".into(), format!("`{}({})`", row.name, row.args), "—".into());
    }
    if let Some((_, target)) = DUPLICATES.iter().find(|(flat, _)| *flat == row.name) {
        return ("Alias".into(), format!("`{}({})`", row.name, row.args), format!("→ `{target}`"));
    }
    (row.category.clone(), format!("`{}({})`", row.name, row.args), "—".into())
}

/// Namespaces in the order `scripting::namespaces::CORE` lists them, then
/// anything unlisted, then the globals and the aliases.
fn namespace_rank(namespace: &str) -> usize {
    use crate::scripting::namespaces::CORE;
    let mut seen: Vec<&str> = Vec::new();
    for (ns, _, _) in CORE {
        if !seen.contains(ns) {
            seen.push(ns);
        }
    }
    match namespace {
        "Global" => seen.len() + 2,
        "Alias" => seen.len() + 3,
        other => seen.iter().position(|n| *n == other).unwrap_or(seen.len() + 1),
    }
}

/// Heading the generated table lives under.
const SECTION: &str = "## API Index";
/// Markers that delimit generated content, so hand-written prose around the
/// table survives regeneration.
const BEGIN: &str = "<!-- BEGIN GENERATED API INDEX -->";
const END: &str = "<!-- END GENERATED API INDEX -->";

/// Render the complete index section body (the marker block's contents).
pub fn render_api_index(inputs: IndexInputs<'_>) -> String {
    let direct = direct_luau_globals(inputs.luau_rs);
    let mut editor = bridge_functions(inputs.lua_runtime_ts);
    editor.extend(lua_side_globals(inputs.lua_runtime_ts));
    let existing = existing_rows(inputs.existing_doc);

    let mut rows: BTreeMap<String, Row> = BTreeMap::new();

    // 1. Registry entries — full signatures from real data.
    for f in inputs.registry.iter() {
        let args = f
            .args
            .iter()
            .map(|a| a.name.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let prior = existing.get(&f.name);
        rows.insert(
            f.name.clone(),
            Row {
                category: prior
                    .map(|r| r.category.clone())
                    .unwrap_or_else(|| category_for(&f.name)),
                args,
                returns: richer_returns(prior.map(|r| r.returns.as_str()), returns_cell(&f.returns)),
                side_effect: prior
                    .map(|r| r.side_effect.clone())
                    .unwrap_or_else(|| default_side_effect(&f.returns)),
                runtimes: String::new(),
                name: f.name.clone(),
            },
        );
    }

    // 2. Globals only the Luau adapter sets. No signature is recoverable from
    //    the source, so keep whatever the table already said.
    for name in &direct {
        if rows.contains_key(name) {
            continue;
        }
        let prior = existing.get(name);
        rows.insert(
            name.clone(),
            Row {
                category: prior
                    .map(|r| r.category.clone())
                    .unwrap_or_else(|| category_for(name)),
                args: prior.map(|r| r.args.clone()).unwrap_or_else(|| "…".into()),
                returns: prior.map(|r| r.returns.clone()).unwrap_or_else(|| "—".into()),
                side_effect: prior
                    .map(|r| r.side_effect.clone())
                    .unwrap_or_else(|| "—".into()),
                runtimes: String::new(),
                name: name.clone(),
            },
        );
    }

    // 3. Availability, from where a function is actually *installed* — not
    //    from the registry. `reg!` records metadata for editor tooltips;
    //    `BridgeRunner::install_script_api` skips descriptors with no
    //    handler, and no core descriptor has one. So the native surface is
    //    the `globals.set(…)` calls in the Luau adapter, and a registry entry
    //    absent from there is documented but not callable on native.
    let mut native_only = 0usize;
    let mut browser_only = 0usize;
    for row in rows.values_mut() {
        let on_native = direct.contains(&row.name);
        let in_browser = editor.contains(&row.name);
        row.runtimes = match (on_native, in_browser) {
            (true, true) => "all".into(),
            (true, false) => {
                native_only += 1;
                "native only".into()
            }
            (false, true) => {
                browser_only += 1;
                "editor + web".into()
            }
            (false, false) => "unreachable".into(),
        };
    }

    let mut out = String::new();
    out.push_str(BEGIN);
    out.push('\n');
    out.push_str(&format!(
        "\n<!-- Generated by `cargo run -p jengine-core --example script_api_index`. Do not edit by hand. -->\n\n\
         {} functions.\n\n\
         **Runtimes** says where a function is actually installed:\n\n\
         - `all` — the Luau adapter and the Fengari runtime the editor and web builds share.\n\
         - `native only` — set on the Luau VM but absent from `BRIDGE_FUNCTIONS` in \
         `editor/src/lib/lua-runtime.ts`, so it is `nil` in the editor and in every web build ({} of these).\n\
         - `editor + web` — registered in `core_script_api.rs` for editor tooltips but never installed \
         on the Luau VM, because `install_script_api` skips descriptors without a handler and no core \
         descriptor has one. Callable in the browser runtimes, `nil` on native ({} of these).\n\
         - `unreachable` — documented and installed nowhere.\n\n\
         **Function** is the name to call - namespaced, as `docs/script-namespaces-plan.md` \
         describes and `scripting::namespaces` lists. **Flat name** is the older global the \
         same function is still installed under; game scripts use the namespaced name \
         (`shipped_scripts_use_namespaces`), JavaScript scripts the flat one. `Global` rows \
         have no namespace; an `Alias` is a second flat name for a function listed elsewhere.\n\n",
        rows.len(),
        native_only,
        browser_only
    ));
    out.push_str("| Namespace | Function | Flat name | Returns | Side effect | Runtimes |\n");
    out.push_str("|---|---|---|---|---|---|\n");

    // Namespace first, then the call: the table is read by section.
    let mut ordered: Vec<(String, String, String, &Row)> = rows
        .values()
        .map(|r| {
            let (ns, call, flat) = presentation(r);
            (ns, call, flat, r)
        })
        .collect();
    ordered.sort_by(|a, b| {
        namespace_rank(&a.0)
            .cmp(&namespace_rank(&b.0))
            .then_with(|| a.0.cmp(&b.0))
            .then_with(|| a.1.cmp(&b.1))
    });
    for (ns, call, flat, row) in ordered {
        out.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} |\n",
            ns, call, flat, row.returns, row.side_effect, row.runtimes
        ));
    }
    out.push('\n');
    out.push_str(END);
    out
}

/// Replace the generated block inside `doc`, inserting it under the
/// `## API Index` heading the first time.
pub fn replace_api_index(doc: &str, generated: &str) -> Option<String> {
    let newline = if doc.contains("\r\n") { "\r\n" } else { "\n" };
    let generated = generated.replace('\n', newline);
    if let (Some(start), Some(end)) = (doc.find(BEGIN), doc.find(END)) {
        let mut out = String::with_capacity(doc.len());
        out.push_str(&doc[..start]);
        out.push_str(&generated);
        out.push_str(&doc[end + END.len()..]);
        return Some(out);
    }
    // First run: drop the block in after the heading's intro paragraph,
    // replacing the old hand-written table (everything up to the next `## `).
    let heading = doc.find(SECTION)?;
    let after_heading = heading + SECTION.len();
    let next_section = doc[after_heading..]
        .find(&format!("{newline}## "))
        .map(|i| after_heading + i)?;
    let mut out = String::with_capacity(doc.len());
    out.push_str(&doc[..after_heading]);
    out.push_str(newline);
    out.push_str(newline);
    out.push_str(&generated);
    out.push_str(newline);
    out.push_str(&doc[next_section..]);
    Some(out)
}

/// Names the Luau adapter binds with `globals.set("…")`.
fn direct_luau_globals(src: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in src.lines() {
        for marker in ["globals.set(", "globals().set("] {
            if let Some(idx) = line.find(marker)
                && let Some(name) = first_quoted(&line[idx + marker.len()..])
            {
                out.insert(name);
            }
        }
    }
    out
}

/// The Fengari allowlist: what the editor and web builds expose.
fn bridge_functions(ts: &str) -> BTreeSet<String> {
    let Some(start) = ts.find("BRIDGE_FUNCTIONS") else {
        return BTreeSet::new();
    };
    // `const BRIDGE_FUNCTIONS: string[] = [` — the first `[` is the type's.
    let Some(eq) = ts[start..].find('=') else {
        return BTreeSet::new();
    };
    let Some(open) = ts[start + eq..].find('[') else {
        return BTreeSet::new();
    };
    // +1 to step past the `[` itself, which would otherwise cling to the
    // first entry when the array opens on the same line.
    let body_start = start + eq + open + 1;
    let Some(close) = ts[body_start..].find(']') else {
        return BTreeSet::new();
    };
    let mut out = BTreeSet::new();
    for chunk in ts[body_start..body_start + close].split([',', '\n']) {
        let t = chunk.trim().trim_matches(['\'', '"', ' ']);
        if is_global_name(t) {
            out.insert(t.to_string());
        }
    }
    out
}

/// Globals `lua-runtime.ts` installs itself, outside the allowlist:
/// `lua_setglobal(L, to_luastring('require'))` and the like. `require` is
/// implemented in Lua-side TypeScript rather than as a bridge function, so
/// reading `BRIDGE_FUNCTIONS` alone called it native-only for as long as the
/// editor and web builds had it.
fn lua_side_globals(ts: &str) -> BTreeSet<String> {
    const CALL: &str = "lua_setglobal(";
    const NAME: &str = "to_luastring(";
    let mut out = BTreeSet::new();
    let mut rest = ts;
    while let Some(at) = rest.find(CALL) {
        rest = &rest[at + CALL.len()..];
        let Some(close) = rest.find(')') else { break };
        let Some(n) = rest[..close].find(NAME) else { continue };
        let t = rest[n + NAME.len()..close].trim().trim_matches(['\'', '"']);
        if is_global_name(t) {
            out.insert(t.to_string());
        }
    }
    out
}

/// Rows already in the document, so hand-written categories and the
/// signatures of registry-less globals survive.
fn existing_rows(doc: &str) -> BTreeMap<String, Row> {
    let mut out = BTreeMap::new();
    for line in doc.lines() {
        let t = line.trim();
        if !t.starts_with('|') || t.starts_with("|-") {
            continue;
        }
        let cols = split_row(t);
        let cols: Vec<&str> = cols.iter().map(|c| c.trim()).collect();
        // Now: "", namespace, function, flat name, returns, side effect, runtimes, ""
        // Before Sep 2026: "", category, function, returns, side effect, [runtimes], ""
        if cols.len() < 6 {
            continue;
        }
        let with_flat = cols.len() >= 8;
        let call = cols[2].trim_matches('`');
        let (head, args) = match (call.find('('), call.rfind(')')) {
            (Some(open), Some(close)) if close > open => (&call[..open], &call[open + 1..close]),
            _ => continue,
        };
        let flat_cell = if with_flat { cols[3].trim_matches('`') } else { "" };
        let name = if is_global_name(flat_cell) { flat_cell } else { head.trim() };
        if !is_global_name(name) {
            continue;
        }
        let (returns, side) = if with_flat { (cols[4], cols[5]) } else { (cols[3], cols[4]) };
        out.insert(
            name.to_string(),
            Row {
                category: cols[1].to_string(),
                args: args.to_string(),
                returns: returns.to_string(),
                side_effect: side.to_string(),
                runtimes: String::new(),
                name: name.to_string(),
            },
        );
    }
    out
}

/// A table row's cells, split on `|` but not on an escaped `\|` - which a
/// returns cell like `entity_id \| nil` needs. Splitting on every pipe used
/// to shift that row's columns, and each regeneration wrote the damage back.
fn split_row(line: &str) -> Vec<String> {
    let mut cells = vec![String::new()];
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek() == Some(&'|') {
            cells.last_mut().unwrap().push_str("\\|");
            chars.next();
        } else if c == '|' {
            cells.push(String::new());
        } else {
            cells.last_mut().unwrap().push(c);
        }
    }
    cells
}

fn first_quoted(text: &str) -> Option<String> {
    let start = text.find('"')?;
    let rest = &text[start + 1..];
    let end = rest.find('"')?;
    let name = &rest[..end];
    is_global_name(name).then(|| name.to_string())
}

fn is_global_name(name: &str) -> bool {
    name.len() > 1
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn returns_cell(t: &ScriptValueType) -> String {
    match t {
        ScriptValueType::Nil => "—".to_string(),
        ScriptValueType::Vec2 => "`x, y`".to_string(),
        ScriptValueType::Rgba => "`r, g, b, a`".to_string(),
        // A `|` in a type (`entity_id | nil`) would end the table cell.
        other => format!("`{}`", other.to_string().replace('|', "\\|")),
    }
}

/// The registry's return type, unless the table already says more than it
/// can. `ScriptValueType` has no way to say "or nil" or to name several
/// returns, so a hand-written `entity_id | nil` or `x, y (screen px)` is kept
/// over the generated `entity_id` / `x, y`, and anything is kept over a bare
/// `...` or `any`. A prior cell that merely differs is replaced: the registry
/// is the source, and a stale hand-written type is how this table drifted.
fn richer_returns(prior: Option<&str>, generated: String) -> String {
    let Some(prior) = prior.map(str::trim).filter(|p| !p.is_empty() && *p != "—") else {
        return generated;
    };
    let bare = |s: &str| s.trim_matches('`').trim().to_string();
    let (p, g) = (bare(prior), bare(&generated));
    let vague = g == "..." || g == "any";
    if p != g && (vague || p.starts_with(&g)) {
        prior.to_string()
    } else {
        generated
    }
}

/// A function that returns nothing is queuing work for the engine to apply
/// later; one that returns a value answered from this frame's snapshot.
fn default_side_effect(t: &ScriptValueType) -> String {
    match t {
        ScriptValueType::Nil => "deferred".to_string(),
        _ => "—".to_string(),
    }
}

/// Category from the name, for functions the old table never listed.
fn category_for(name: &str) -> String {
    const PREFIXES: &[(&str, &str)] = &[
        ("save_", "Save"),
        ("input_", "Input"),
        ("audio_", "Audio"),
        ("tilemap_", "Tilemap"),
        ("gui_", "GUI"),
        ("ui_", "UI"),
        ("anim", "Animation"),
        ("set_anim", "Animation"),
        ("particle", "Particles"),
        ("debug_", "Debug"),
        ("timer_", "Timers"),
        ("event_", "Events"),
        ("physics_", "Physics"),
        ("camera_", "Camera"),
        ("material_", "Material"),
        ("light_", "Lighting"),
    ];
    for (prefix, cat) in PREFIXES {
        if name.starts_with(prefix) {
            return (*cat).to_string();
        }
    }
    const CONTAINS: &[(&str, &str)] = &[
        ("world_", "Hierarchy"),
        ("parent", "Hierarchy"),
        ("skeleton", "Skeletal"),
        ("bone", "Skeletal"),
        ("rig", "Skeletal"),
        ("_ik_", "Skeletal"),
        ("sound", "Audio"),
        ("music", "Audio"),
        ("scene", "Scene"),
        ("sprite", "Sprite"),
        ("screen", "Camera"),
        ("event", "Events"),
    ];
    for (needle, cat) in CONTAINS {
        if name.contains(needle) {
            return (*cat).to_string();
        }
    }
    "Misc".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::script_api::{ScriptArg, ScriptValueType};

    fn registry_with(name: &str, args: &[&str], ret: ScriptValueType) -> ScriptApiRegistry {
        let mut api = ScriptApiRegistry::new();
        api.add_function(
            name,
            "test",
            "desc",
            args.iter()
                .map(|a| ScriptArg {
                    name: (*a).to_string(),
                    arg_type: ScriptValueType::Float,
                })
                .collect(),
            ret,
        );
        api
    }

    #[test]
    fn registry_functions_render_with_their_signature() {
        let api = registry_with("set_thing", &["entity_id", "value"], ScriptValueType::Nil);
        let table = render_api_index(IndexInputs {
            registry: &api,
            luau_rs: r#"globals.set("set_thing", f)?;"#,
            lua_runtime_ts: "const BRIDGE_FUNCTIONS: string[] = ['set_thing'];",
            existing_doc: "",
        });
        assert!(table.contains("`set_thing(entity_id, value)`"), "{table}");
        assert!(table.contains("| all |"), "{table}");
    }

    #[test]
    fn a_luau_global_missing_from_the_allowlist_is_native_only() {
        let api = registry_with("lonely_fn", &[], ScriptValueType::Nil);
        let table = render_api_index(IndexInputs {
            registry: &api,
            luau_rs: r#"globals.set("lonely_fn", f)?;"#,
            lua_runtime_ts: "const BRIDGE_FUNCTIONS: string[] = ['something_else'];",
            existing_doc: "",
        });
        assert!(table.contains("| native only |"), "{table}");
    }

    #[test]
    fn a_registry_only_function_is_not_claimed_to_work_on_native() {
        // `reg!` records metadata; `install_script_api` skips descriptors
        // with no handler, so a registry entry the Luau adapter never sets is
        // documented but not callable there. Reporting it as native is the
        // mistake that hid 21 browser-only functions.
        let api = registry_with("browser_side", &[], ScriptValueType::Nil);
        let table = render_api_index(IndexInputs {
            registry: &api,
            luau_rs: "",
            lua_runtime_ts: "const BRIDGE_FUNCTIONS: string[] = ['browser_side'];",
            existing_doc: "",
        });
        assert!(table.contains("| editor + web |"), "{table}");
        assert!(!table.contains("| all |"), "{table}");
    }

    #[test]
    fn a_global_the_lua_runtime_sets_itself_counts_as_the_browsers() {
        // `require` is installed with `lua_setglobal`, not through the
        // allowlist, and the index called it native-only because of it.
        let api = registry_with("require", &[], ScriptValueType::Nil);
        let table = render_api_index(IndexInputs {
            registry: &api,
            luau_rs: r#"globals.set("require", f)?;"#,
            lua_runtime_ts: "const BRIDGE_FUNCTIONS: string[] = [];
                             lua.lua_setglobal(L, to_luastring('require'));",
            existing_doc: "",
        });
        assert!(table.contains("| all |"), "{table}");
    }

    #[test]
    fn a_function_installed_nowhere_is_marked_unreachable() {
        let api = registry_with("ghost", &[], ScriptValueType::Nil);
        let table = render_api_index(IndexInputs {
            registry: &api,
            luau_rs: "",
            lua_runtime_ts: "const BRIDGE_FUNCTIONS: string[] = [];",
            existing_doc: "",
        });
        assert!(table.contains("| unreachable |"), "{table}");
    }

    #[test]
    fn a_hand_written_return_that_says_more_is_kept() {
        let render = |ret: ScriptValueType, existing: &str| {
            render_api_index(IndexInputs {
                registry: &registry_with("find_thing", &["name"], ret),
                luau_rs: r#"globals.set("find_thing", f)?;"#,
                lua_runtime_ts: "const BRIDGE_FUNCTIONS: string[] = ['find_thing'];",
                existing_doc: existing,
            })
        };
        // "or nil" extends the registry's type: kept.
        let table = render(ScriptValueType::EntityId, "| Entity | `find_thing(name)` | `entity_id \\| nil` | — | all |");
        assert!(table.contains("`entity_id \\| nil`"), "{table}");
        // The registry can only say `...`: the named returns are kept.
        let table = render(ScriptValueType::VarArgs, "| Physics | `find_thing(name)` | `a, b, started` | — | all |");
        assert!(table.contains("`a, b, started`"), "{table}");
        // A stale type that merely differs is replaced by the registry's.
        let table = render(ScriptValueType::Float, "| Entity | `find_thing(name)` | `string` | — | all |");
        assert!(table.contains("`float`") && !table.contains("`string`"), "{table}");
    }

    #[test]
    fn direct_luau_globals_keep_the_documents_signature() {
        // No registry entry exists for these, so the old table is the only
        // place their argument list is written down.
        let api = ScriptApiRegistry::new();
        let table = render_api_index(IndexInputs {
            registry: &api,
            luau_rs: r#"globals.set("screen_flash", f)?;"#,
            lua_runtime_ts: "const BRIDGE_FUNCTIONS: string[] = [];",
            existing_doc: "| Camera | `screen_flash(r, g, b, a, duration)` | — | deferred | all |",
        });
        // `screen_flash` is `Camera.flash` in the namespace table.
        assert!(table.contains("`Camera.flash(r, g, b, a, duration)`"), "{table}");
        assert!(table.contains("| Camera |"), "{table}");
        assert!(table.contains("| `screen_flash` |"), "{table}");
    }

    #[test]
    fn a_namespaced_function_is_listed_by_its_namespaced_name() {
        let api = registry_with("set_sprite_color", &["entity_id", "r"], ScriptValueType::Nil);
        let table = render_api_index(IndexInputs {
            registry: &api,
            luau_rs: r#"globals.set("set_sprite_color", f)?;"#,
            lua_runtime_ts: "const BRIDGE_FUNCTIONS: string[] = ['set_sprite_color'];",
            existing_doc: "",
        });
        assert!(
            table.contains("| Sprite | `Sprite.set_color(entity_id, r)` | `set_sprite_color` |"),
            "{table}"
        );
    }

    #[test]
    fn a_row_in_the_new_layout_reads_back() {
        // The generator re-reads its own table for signatures no registry has.
        let api = ScriptApiRegistry::new();
        let table = render_api_index(IndexInputs {
            registry: &api,
            luau_rs: r#"globals.set("screen_flash", f)?;"#,
            lua_runtime_ts: "const BRIDGE_FUNCTIONS: string[] = [];",
            existing_doc: "| Camera | `Camera.flash(duration, r)` | `screen_flash` | — | deferred | all |",
        });
        assert!(table.contains("`Camera.flash(duration, r)`"), "{table}");
    }

    #[test]
    fn replace_is_idempotent() {
        // The markers come from the renderer, so a realistic block carries
        // them; that is what lets a second call find and swap the block
        // rather than insert another one.
        let block = format!("{BEGIN}
TABLE
{END}");
        let doc = "# Doc

## API Index

old hand-written table

## Next
";
        let first = replace_api_index(doc, &block).expect("insert");
        assert!(first.contains(BEGIN) && first.contains("TABLE"), "{first}");
        assert!(first.contains("## Next"), "{first}");
        assert!(!first.contains("old hand-written table"), "{first}");
        let second = replace_api_index(&first, &block).expect("replace");
        assert_eq!(first, second);
    }
}
