//! Every field the browser preamble reads off `window.__jengine_play.state`
//! must be published by both browser hosts.
//!
//! # Why a source scan
//!
//! Getters cannot be commands - Lua has no await, so `get_velocity`,
//! `get_gravity`, `get_contacts` and friends answer from a `state` object the
//! host refreshes each frame. A field the host never sets is not an error: the
//! preamble falls back to a default (`[0, 0]`, `[]`, `null`) and the script
//! carries on with a wrong answer. The name-level parity checks all pass,
//! because the global exists.
//!
//! `velocities` was exactly that until BUGS #91. `get_velocity` answered
//! `(0, 0)` for every entity in the editor and on the web, so test-lab scene
//! 17's player - which reads its own `vy` back each frame to keep gravity -
//! cancelled its own fall and drifted down while every other body dropped.

const PREAMBLE: &str = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
const NAV: &str = include_str!("../src/scripting/bridge/adapters/js/nav.rs");
const RAY: &str = include_str!("../src/scripting/bridge/adapters/js/ray.rs");
/// The `Locale.*` hooks, installed with the nav ones (`install_nav_callbacks`).
const LOCALE: &str = include_str!("../src/scripting/bridge/adapters/js/locale.rs");
/// The `Input.rumble` / `vibrate` hooks, installed with the nav ones too.
const HAPTICS: &str = include_str!("../src/scripting/bridge/adapters/js/haptics.rs");
/// The `Display.*` / `Cursor.*` hook, installed with the nav ones too.
const DISPLAY: &str = include_str!("../src/scripting/bridge/adapters/js/display.rs");
/// The `Scene.load_additive` hook, likewise.
const STREAMING: &str = include_str!("../src/scripting/bridge/adapters/js/streaming.rs");
/// The `Packs.*` hook, likewise.
const PACKS: &str = include_str!("../src/scripting/bridge/adapters/js/packs.rs");
/// The `LiveOps.*` hook, likewise.
const LIVEOPS: &str = include_str!("../src/scripting/bridge/adapters/js/liveops.rs");
/// The hold / tap / prompt hook (`input_query`), likewise.
const INPUT_QUERY: &str = include_str!("../src/scripting/bridge/adapters/js/input_query.rs");
/// The music director's hook (`music`), likewise.
const MUSIC: &str = include_str!("../src/scripting/bridge/adapters/js/music.rs");
const NOISE: &str = include_str!("../src/scripting/bridge/adapters/js/noise.rs");
const ANALYSIS: &str = include_str!("../src/scripting/bridge/adapters/js/analysis.rs");
const SCREENSHOT: &str = include_str!("../src/scripting/bridge/adapters/js/screenshot.rs");
const ACCESSIBILITY: &str = include_str!("../src/scripting/bridge/adapters/js/accessibility.rs");
/// What both hosts publish through one shared function (`publish_tilemaps`,
/// `publish_kids`) - counted for a host only if it calls that function.
const PUBLISH: &str = include_str!("../src/scripting/bridge/adapters/js/publish.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB_LIB: &str = include_str!("../../jengine-web/src/lib.rs");
const WEB_HOST: &str = include_str!("../../jengine-web/src/script_host.rs");

/// Fields the preamble reads that no host publishes yet. May only shrink.
///
/// **Empty since Sep 2026.** `physics_raycast` was the last one, and it had
/// been here for the whole life of this test: a ray needs a synchronous
/// callback into rapier, and the hosts' `RefCell`-owned world cannot answer
/// one from inside a script, so the preamble's `raycast` returned `null`
/// while every name-level check counted the global as present.
///
/// `adapters/js/ray.rs` installs it the way `nav.rs` installs pathfinding -
/// the callback owns a snapshot of the colliders rather than reaching back
/// for the app - and `raycast_snapshot_matches_rapier` is what keeps that
/// snapshot honest, by firing 216 rays at five shapes and requiring the
/// same entity, distance and normal from both it and the live world.
const KNOWN_UNPUBLISHED: &[&str] = &[];

/// Every `s.<field>` read inside a function that binds
/// `var s = window.__jengine_play.state`.
fn fields_read() -> Vec<String> {
    let mut out = Vec::new();
    for (start, _) in PREAMBLE.match_indices("var s = window.__jengine_play.state") {
        let body = &PREAMBLE[start..];
        // To the end of the enclosing function: a one-liner ends where the
        // next function starts, a multi-line one at its closing brace.
        let end = ["\n  function ", "\n  }}"]
            .iter()
            .filter_map(|stop| body.find(stop))
            .min()
            .unwrap_or(body.len());
        let body = &body[..end];
        for (i, _) in body.match_indices("s.") {
            // `s.` must start an identifier, not end one (`vs.x`, `rows.`).
            if body[..i]
                .chars()
                .last()
                .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
            {
                continue;
            }
            let field: String = body[i + 2..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !field.is_empty() && !out.contains(&field) {
                out.push(field);
            }
        }
    }
    out
}

/// Every `p.<field>` read inside a function that binds
/// `var p = window.__jengine_play`. The same failure one level up: the
/// getters that answer from the frame's own numbers - the mouse, the
/// screen, the camera - read the play object directly rather than its
/// `state` child, and a field no host sets falls back just as silently.
/// `camera_rot` was one, added when the camera learned to turn.
fn play_fields_read() -> Vec<String> {
    let mut out = Vec::new();
    for (start, _) in PREAMBLE.match_indices("var p = window.__jengine_play;") {
        let body = &PREAMBLE[start..];
        let end = ["\n  function ", "\n  }}"]
            .iter()
            .filter_map(|stop| body.find(stop))
            .min()
            .unwrap_or(body.len());
        let body = &body[..end];
        for (i, _) in body.match_indices("p.") {
            if body[..i]
                .chars()
                .last()
                .is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '.')
            {
                continue;
            }
            let field: String = body[i + 2..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !field.is_empty() && !out.contains(&field) {
                out.push(field);
            }
        }
    }
    out
}

/// Whether `field` is set by a function in `publish.rs` that `host` calls.
fn shared_publisher_called(field: &str, host: &str) -> bool {
    let needle = format!("JsValue::from_str(\"{field}\")");
    let Some(at) = PUBLISH.find(&needle) else {
        return false;
    };
    let Some(start) = PUBLISH[..at].rfind("pub fn ") else {
        return false;
    };
    let name: String = PUBLISH[start + 7..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
    host.contains(&format!("publish::{name}("))
}


/// The per-frame publish both browser hosts share (`adapters::js::mirror`).
const MIRROR: &str = include_str!("../src/scripting/bridge/adapters/js/mirror.rs");

/// A host's source as far as what it publishes goes: its own, plus the shared
/// mirror - but only if it calls it, so a host that dropped the call fails.
fn with_mirror(src: &str) -> String {
    if src.contains("mirror::sync_world_to_play_namespace(") {
        format!("{src}
{MIRROR}")
    } else {
        src.to_string()
    }
}

#[test]
fn the_shared_publishers_are_counted() {
    for host in [with_mirror(VIEWPORT), with_mirror(WEB_HOST)] {
        let host = host.as_str();
        assert!(shared_publisher_called("kids", host), "a host no longer calls publish_kids");
        assert!(shared_publisher_called("tilemaps", host), "a host no longer calls publish_tilemaps");
        assert!(shared_publisher_called("names", host), "a host no longer calls publish_names");
        assert!(shared_publisher_called("velocities", host), "a host no longer calls publish_velocities");
    }
}

#[test]
fn the_scan_finds_the_state_getters() {
    let fields = fields_read();
    for expected in ["velocities", "angular_velocities", "gravity", "contacts", "tilemap_find_path"] {
        assert!(
            fields.iter().any(|f| f == expected),
            "the preamble scan no longer finds `{expected}` - it found {fields:?}. \
             Fix the scan before trusting the parity check below."
        );
    }
}

#[test]
fn both_browser_hosts_publish_every_state_field_the_preamble_reads() {
    let hosts = [
        ("editor viewport", [&with_mirror(VIEWPORT), NAV, RAY, LOCALE, HAPTICS, DISPLAY, STREAMING, PACKS, LIVEOPS, INPUT_QUERY, MUSIC, NOISE, ANALYSIS, SCREENSHOT, ACCESSIBILITY].concat()),
        ("web", [WEB_LIB, &with_mirror(WEB_HOST), NAV, RAY, LOCALE, HAPTICS, DISPLAY, STREAMING, PACKS, LIVEOPS, INPUT_QUERY, MUSIC, NOISE, ANALYSIS, SCREENSHOT, ACCESSIBILITY].concat()),
    ];
    for field in fields_read() {
        let needle = format!("JsValue::from_str(\"{field}\")");
        for (name, src) in &hosts {
            // Velocities go out through `publish::publish_velocities`.
            let published = src.contains(&needle) || shared_publisher_called(&field, src);
            let known = KNOWN_UNPUBLISHED.contains(&field.as_str());
            assert!(
                published || known,
                "the preamble reads `__jengine_play.state.{field}` but the {name} host \
                 never sets it, so the getter silently returns its fallback there"
            );
            assert!(
                !(published && known),
                "`{field}` is published by the {name} host now - delete it from \
                 KNOWN_UNPUBLISHED and mark the row FIXED in docs/parity-gaps.md"
            );
        }
    }
}

#[test]
fn the_play_scan_finds_the_frame_getters() {
    let fields = play_fields_read();
    for expected in ["mouse_x", "screen_w", "camera_zoom", "camera_rot"] {
        assert!(
            fields.iter().any(|f| f == expected),
            "the play-object scan no longer finds `{expected}` - it found {fields:?}. \
             Fix the scan before trusting the parity check below."
        );
    }
}

#[test]
fn both_browser_hosts_publish_every_play_field_the_preamble_reads() {
    let hosts = [
        ("editor viewport", [&with_mirror(VIEWPORT), NAV, RAY, LOCALE, HAPTICS, DISPLAY, STREAMING, PACKS, LIVEOPS, INPUT_QUERY, MUSIC, NOISE, ANALYSIS, SCREENSHOT, ACCESSIBILITY].concat()),
        ("web", [WEB_LIB, &with_mirror(WEB_HOST), NAV, RAY, LOCALE, HAPTICS, DISPLAY, STREAMING, PACKS, LIVEOPS, INPUT_QUERY, MUSIC, NOISE, ANALYSIS, SCREENSHOT, ACCESSIBILITY].concat()),
    ];
    for field in play_fields_read() {
        // `state` is the child object the check above covers.
        if field == "state" {
            continue;
        }
        // A field the preamble writes itself (its sound-id counter, `__sid`)
        // is the preamble's own: no host is meant to publish it.
        if PREAMBLE.contains(&format!("p.{field} = ")) {
            continue;
        }
        let needle = format!("JsValue::from_str(\"{field}\")");
        for (name, src) in &hosts {
            assert!(
                src.contains(&needle) || shared_publisher_called(field.as_str(), src),
                "the preamble reads `__jengine_play.{field}` but the {name} host never \
                 sets it, so the getter silently returns its fallback there"
            );
        }
    }
}

/// The hosts publish velocities by entity id - `[vx, vy]` at `id * 2` and
/// `omega` at `id` in `Float32Array`s, NaN where an entity has none
/// (`publish::publish_velocities`) - so a getter reads one slot instead of
/// scanning a row per body. This runs the preamble's two getters, lifted out
/// of `browser.rs` with `__ent`, under `node` against that shape, a spawn
/// handle (1000001, entity 3) among them. Skipped without `node`.
#[test]
fn the_preamble_reads_velocities_by_id() {
    const SRC: &str = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
    let lift = |name: &str| -> String {
        let start = SRC.find(&format!("  function {name}(")).unwrap_or_else(|| panic!("{name} is in the preamble"));
        let end = start + SRC[start..].find("\n  }}\n").expect("the function ends") + "\n  }}".len();
        SRC[start..end].replace("{{", "{").replace("}}", "}")
    };
    let script = format!(
        "var window = {{ __jengine_play: {{ alias: {{ 1000001: 3 }}, state: {{\n\
           velocities: new Float32Array([NaN, NaN, 0, 0, 1.5, -2, 3, 4]),\n\
           angular_velocities: new Float32Array([NaN, NaN, 0.5, -1.25]) }} }} }};\n\
         {}\n{}\n{}\n\
         console.log(JSON.stringify([get_velocity(2), get_velocity(1000001), get_velocity(0), get_velocity(5),\n\
           get_angular_velocity(1000001), get_angular_velocity(0), get_angular_velocity(9)]));",
        lift("__ent"),
        lift("get_velocity"),
        lift("get_angular_velocity"),
    );
    let out = match std::process::Command::new("node").arg("-e").arg(&script).output() {
        Ok(out) => out,
        Err(_) => {
            eprintln!("the_preamble_reads_velocities_by_id: no node - skipped");
            return;
        }
    };
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        text.trim(),
        "[[1.5,-2],[3,4],[0,0],[0,0],-1.25,0,0]",
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Both browser hosts publish through the shared mirror and read scripts'
/// writes back through it - the counting above trusts this.
#[test]
fn both_browser_hosts_use_the_shared_mirror() {
    for (name, src) in [("editor viewport", VIEWPORT.to_string()), ("web", format!("{WEB_HOST}{WEB_LIB}"))] {
        for call in [
            "mirror::sync_world_to_play_namespace(",
            "mirror::sync_play_namespace_to_world(",
            "mirror::publish_physics_for_scripts(",
        ] {
            assert!(src.contains(call), "{name} no longer calls `{call}` - its scripts would read a stale or empty world");
        }
    }
}
