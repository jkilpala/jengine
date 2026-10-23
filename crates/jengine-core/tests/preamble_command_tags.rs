//! The browser preamble's command tags must each have exactly one owner.
//!
//! Every script global in `adapters/js/browser.rs` pushes a command tagged with
//! a short string (`t: 'ts'`), and each runtime dispatches on that tag with a
//! `match`. Nothing declares the tag namespace, so reusing one is easy and
//! nearly silent: only the runtime whose arm happens to come *second* gets an
//! "unreachable pattern" warning, and in the others the first arm simply wins
//! and the older function quietly stops working. `set_time_scale` shipped as
//! `ts`, which `tween_scale` already owned; it was caught by luck.
//!
//! This is an integration test reading the source text rather than a unit test
//! calling the builder, for two reasons: the preamble is
//! `#[cfg(target_arch = "wasm32")]` and its module needs the `web` /
//! `js-adapter-host` feature, so anything inside it is invisible to a plain
//! `cargo test` — which is exactly how the clash got as far as it did.

const BROWSER_RS: &str =
    include_str!("../src/scripting/bridge/adapters/js/browser.rs");

/// Tags deliberately shared by several globals, where one runtime arm handles
/// all of them and discriminates on the payload (a uniform's width, a log's
/// severity). Sharing is fine when it is *intended*; this list is what makes
/// the difference between intended and accidental checkable. Adding to it
/// should be a conscious act, not something a new function does by collision.
const INTENTIONALLY_SHARED: &[&str] = &[
    "sc", "log", "smu", "sppp",
    // 2.5 input remapping: six setters share one tag and discriminate on
    // `op`, rather than burning six tags on one feature.
    "ib",
    // 2.4 audio busses: five setters, one tag, `op` discriminates.
    "ab",
    // `send_event` broadcasts and `send_event_to` targets, on one tag: the
    // dispatch is identical apart from filtering the listener list by the
    // `tgt` field, so a second tag would mean a second copy of it.
    "evt",
    // Animation layers: play / stop / weight, one tag, `op` discriminates
    // (`SkeletalPlayer::layer_op`).
    "anly",
    // `paint_sprite` and `press_sprite`: one stamp command, `dp` says which.
    "spnt",
    // Dialogue: start / advance / choose / stop, one tag, `op` discriminates
    // (`ecs::dialogue::op`).
    "dlg",
];

#[test]
fn command_tags_have_exactly_one_owner() {
    let mut owner: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    let mut clashes: Vec<String> = Vec::new();
    let mut current = String::from("<top level>");
    for line in BROWSER_RS.lines() {
        let t = line.trim();
        // JS declarations inside the preamble; Rust ones read `fn` / `pub fn`.
        if let Some(rest) = t.strip_prefix("function ")
            && let Some(paren) = rest.find('(')
        {
            current = rest[..paren].to_string();
        }
        let Some(idx) = t.find("t: '") else { continue };
        let rest = &t[idx + 4..];
        let Some(end) = rest.find('\'') else { continue };
        let tag = rest[..end].to_string();
        match owner.get(&tag) {
            Some(prev) if *prev != current => {
                if !INTENTIONALLY_SHARED.contains(&tag.as_str()) {
                    clashes.push(format!("tag '{tag}': {prev} and {current}"));
                }
            }
            _ => {
                owner.insert(tag, current.clone());
            }
        }
    }
    assert!(
        owner.len() > 50,
        "only {} command tags found — the scan no longer matches the preamble",
        owner.len()
    );
    assert!(
        clashes.is_empty(),
        "command tags reused by different script globals — the runtimes' match \
         arms will shadow each other:\n  {}",
        clashes.join("\n  ")
    );
}

/// Every `__jengine_*` name in the preamble must be reached through `window`.
///
/// The preamble runs as a function inside the host page, so its shared state
/// is only reachable as `window.__jengine_play`. A bare `__jengine_cmds` looks
/// just as plausible and is a `ReferenceError` the moment it runs. That is
/// exactly how `set_ui_prop` shipped: it pushed to `__jengine_cmds`, which
/// nothing defines, so every call threw and aborted the rest of the script's
/// `update` in the editor and on web - while native, which never runs this
/// file, worked. No name-level check can see it; the function binds fine.
#[test]
fn preamble_globals_are_reached_through_window() {
    let mut bare = Vec::new();
    for (i, line) in BROWSER_RS.lines().enumerate() {
        let code = line.trim_start();
        if code.starts_with("//") {
            continue;
        }
        let bytes = line.as_bytes();
        let mut from = 0;
        while let Some(pos) = line[from..].find("__jengine_") {
            let at = from + pos;
            // `.x` is property access; a quote means a Rust string literal
            // naming the property from the host side.
            let prev = if at == 0 { b' ' } else { bytes[at - 1] };
            if !matches!(prev, b'.' | b'"' | b'\'' | b'`') {
                bare.push(format!("line {}: {}", i + 1, line.trim()));
            }
            from = at + "__jengine_".len();
        }
    }
    assert!(
        bare.is_empty(),
        "bare `__jengine_*` references in browser.rs - use `window.__jengine_play...`:\n{}",
        bare.join("\n")
    );
}
