//! `Entity.find_by_name` answers nil for a name nothing has, in every
//! runtime - native Luau and the browser preamble both browsers run.
//!
//! The browser's used to be an alias of `get_entity_by_name`, which answers
//! -1, so a loop ending at `if e == nil then break end` never ended in the
//! editor or a web build: one game's slot scan in `init` spun forever on
//! Play and froze the whole editor, while the same script ran natively.
//! `get_entity_by_name` keeps its -1 everywhere, on purpose (see luau.rs).

/// The preamble's two lookups, run under node: `find_by_name` gives `null`
/// (which Fengari hands Lua as nil), `get_entity_by_name` -1.
#[test]
fn the_browser_preamble_answers_null() {
    const SRC: &str = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
    let lift = |name: &str| -> String {
        let start = SRC.find(&format!("  function {name}(")).unwrap_or_else(|| panic!("{name} is in the preamble"));
        let end = start + SRC[start..].find("\n  }}").unwrap() + 4;
        SRC[start..end].replace("{{", "{").replace("}}", "}")
    };
    let script = format!(
        "var window = {{ __jengine_play: {{ names: {{ Hero: 7 }} }} }};\n{}\n{}\n\
         console.log(JSON.stringify([find_by_name('Hero'), find_by_name('Nobody'), get_entity_by_name('Nobody')]));",
        lift("find_by_name"),
        lift("get_entity_by_name"),
    );
    let out = match std::process::Command::new("node").arg("-e").arg(&script).output() {
        Ok(out) => out,
        Err(_) => {
            eprintln!("the_browser_preamble_answers_null: no node - skipped");
            return;
        }
    };
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(text.trim(), "[7,null,-1]", "{text}\n{}", String::from_utf8_lossy(&out.stderr));
}

/// Fengari turns a JS null into nil, so the loop that `init` runs
/// ends - checked against `lua-runtime.ts`'s conversion, which is what
/// carries the preamble's answer into Lua in both browsers.
#[test]
fn the_editor_lua_runtime_hands_null_to_lua_as_nil() {
    const RUNTIME: &str = include_str!("../../../editor/src/lib/lua-runtime.ts");
    let start = RUNTIME.find("function jsToLua(").expect("jsToLua is in lua-runtime.ts");
    let body = &RUNTIME[start..start + 300];
    assert!(
        body.contains("value === undefined || value === null") && body.contains("lua_pushnil"),
        "jsToLua no longer pushes nil for null:\n{body}"
    );
}
