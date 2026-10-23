//! Helpers shared by the guards that read a runtime host's source.
#![allow(dead_code)]

/// What every host draws a frame with since Oct 2026 (`ecs::frame_draw`):
/// views, world, post chain and UI.
pub const FRAME_DRAW: &str = include_str!("../../src/ecs/frame_draw.rs");

/// Where the frame's shared systems live (`ecs::systems`).
const SYSTEMS: &str = include_str!("../../src/ecs/systems.rs");

/// The steps of a frame every host calls from core since Oct 2026, by name.
/// A guard reading a host for one of the calls inside them reads through the
/// host's call to the function.
const SHARED_STEPS: &[&str] = &[
    "tick_combat_and_animation",
    "physics_frame_begin",
    "physics_fixed_step",
    "physics_frame_end",
    "pose_world_for_draw",
];

/// The body of `pub fn name` in `src`, from its signature to its closing brace.
fn body_of<'a>(src: &'a str, name: &str) -> &'a str {
    let at = src.find(&format!("pub fn {name}(")).unwrap_or_else(|| panic!("no `pub fn {name}` in core"));
    let end = at + src[at..].find("\n}\n").expect("end of the function") + 3;
    &src[at..end]
}

/// `src` with `inline` put in front of its first call of `name` (a call: not
/// the definition), or `src` as it was when it never calls it.
fn inline_at_call(src: &str, name: &str, inline: &str) -> String {
    let needle = format!("{name}(");
    let mut from = 0;
    while let Some(i) = src[from..].find(&needle) {
        let at = from + i;
        let before = &src[..at];
        let defines = before.ends_with("fn ");
        let longer = before.chars().last().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_');
        if !defines && !longer {
            return format!("{}{}\n{}", &src[..at], inline, &src[at..]);
        }
        from = at + needle.len();
    }
    src.to_string()
}

/// A host's source with what it calls from core inlined at the call: the
/// frame draw (`ecs::frame_draw::draw_frame`) and the shared frame steps
/// above. So a check of what a host runs - and in what order - reads through
/// the call as if the host still carried the code. A host that stopped
/// calling one is returned without it, and fails whatever it no longer does.
pub fn with_frame_draw(src: &str) -> String {
    let mut out = match src.find("draw_frame(\n") {
        Some(at) => format!("{}{}\n{}", &src[..at], FRAME_DRAW, &src[at..]),
        None => src.to_string(),
    };
    for name in SHARED_STEPS {
        out = inline_at_call(&out, name, body_of(SYSTEMS, name));
    }
    out
}
