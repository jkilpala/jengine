//! Every command the browser preamble pushes must be handled by **both**
//! browser hosts.
//!
//! `script_global_parity` checks that a function *exists* in each runtime.
//! That is not the same as it working. This session found four functions that
//! were bound, callable, and did nothing:
//!
//! * `send_event` — the web host had no `evt` arm, so no listener ever fired
//!   in a deployed build.
//! * `set_material_uniform_*` — no `smu` arm on web; uniforms applied in the
//!   editor and were ignored in the shipped game.
//! * `add_joint` / `remove_joint` / `set_motor_speed` — no `aj` / `rj` / `sms`
//!   arms on web.
//! * `get_gravity` / `get_contacts` — read a `state` object no host wrote.
//!
//! Each was invisible to every name-level check, and each was found only by
//! reading the dispatch by hand. This test reads it mechanically instead: for
//! every `{ t: '<tag>' }` the preamble emits, both hosts must have a matching
//! `"<tag>" =>` arm.
//!
//! It cannot prove an arm is *correct* — only that the command is not dropped
//! on the floor, which is the failure that actually happened, twice, in
//! functions that had shipped months earlier.
//!
//! Since Sep 2026 an arm may also live in
//! `jengine_core::scripting::host_commands`, shared by both hosts rather than
//! copied into each. Those count as handled — `host_command_parity.rs` is what
//! checks the hosts still call the shared dispatcher, without which they would
//! silently be handling none of them.

const BROWSER_RS: &str = include_str!("../src/scripting/bridge/adapters/js/browser.rs");

/// Commands a host currently drops, with what that breaks.
///
/// A **ratchet**, like `script_global_parity`: the list may only shrink,
/// and anything new fails. Every entry is a script function that is
/// callable and does nothing in the named runtime — all fourteen were
/// found by this test on its first run, having shipped that way for
/// months.
///
/// Format: `(tag, function, hosts that drop it, what breaks)`.
const KNOWN_UNHANDLED: &[(&str, &str, Hosts, &str)] = &[
    // Empty as of 2026-09-02: all fourteen this test found on its first run
    // have been closed. An entry here is a script function that is callable
    // and does nothing, so add one only with what it breaks — never to
    // silence a failure.
];

#[derive(Clone, Copy, PartialEq)]
enum Hosts {
    /// Missing from the editor viewport and the web runtime.
    Both,
    /// Missing from the web runtime only. These survive because the editor
    /// is exercised daily and web builds are not. No gap of this kind is
    /// open right now; the variant stays for the next one.
    #[allow(dead_code)]
    Web,
}

fn preamble_tags() -> std::collections::BTreeSet<String> {
    let mut tags = std::collections::BTreeSet::new();
    for line in BROWSER_RS.lines() {
        let mut rest = line;
        // A line can push more than one command (the compact one-liners).
        while let Some(idx) = rest.find("t: '") {
            let after = &rest[idx + 4..];
            // `ut: 'f'` (a uniform's type) is not a tag.
            if rest[..idx].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
                rest = after;
                continue;
            }
            if let Some(end) = after.find('\'') {
                let tag = &after[..end];
                if !tag.is_empty() && tag.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
                    tags.insert(tag.to_string());
                }
                rest = &after[end..];
            } else {
                break;
            }
        }
    }
    tags
}

/// Read a host's dispatch source. Not `include_str!`: the hosts live in other
/// crates, and a partial checkout should fail with a sentence.
fn host_source(relative: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "cannot read {} ({e}). This test compares the browser preamble \
             against both host dispatchers and needs the full workspace.",
            path.display()
        )
    })
}

/// Tags a host has a match arm for, including `"a" | "b" =>` alternations.
fn handled_tags(src: &str) -> std::collections::BTreeSet<String> {
    let mut tags = std::collections::BTreeSet::new();
    for line in src.lines() {
        let trimmed = line.trim();
        let Some(arrow) = trimmed.find("=>") else {
            continue;
        };
        let head = &trimmed[..arrow];
        if !head.contains('"') {
            continue;
        }
        // `"gs" | "gh" =>` — take every quoted literal on the left.
        let mut rest = head;
        while let Some(open) = rest.find('"') {
            let after = &rest[open + 1..];
            let Some(close) = after.find('"') else { break };
            let tag = &after[..close];
            if !tag.is_empty()
                && tag.len() <= 6
                && tag
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            {
                tags.insert(tag.to_string());
            }
            rest = &after[close + 1..];
        }
    }
    tags
}

#[test]
fn every_preamble_command_has_an_arm_in_both_hosts() {
    let tags = preamble_tags();
    assert!(
        tags.len() > 60,
        "only found {} command tags in the preamble — the scrape has stopped \
         matching and this test is no longer checking anything",
        tags.len()
    );

    let mut viewport = handled_tags(&host_source("../jengine-editor-viewport/src/lib.rs"));
    let mut web = handled_tags(&host_source("../jengine-web/src/script_host.rs"));
    // Commands implemented once in `scripting::host_commands` and dispatched by
    // both hosts before their own `match`. The arm is not in either host file,
    // which is the point.
    for tag in jengine_core::scripting::host_commands::SHARED_WORLD_TAGS {
        viewport.insert(tag.to_string());
        web.insert(tag.to_string());
    }
    // And the shared arms that need more than the world
    // (`adapters::js::host_dispatch`), for a host that calls them.
    for (src, set) in [("../jengine-editor-viewport/src/lib.rs", &mut viewport), ("../jengine-web/src/script_host.rs", &mut web)] {
        if host_source(src).contains("host_dispatch::handle_host_command(") {
            set.extend(jengine_core::scripting::host_commands::HOST_DISPATCH_TAGS.iter().map(|t| t.to_string()));
        }
    }
    // A `Both` entry excuses the viewport too; a `Web` entry does not.
    let excused_viewport: std::collections::BTreeSet<&str> = KNOWN_UNHANDLED
        .iter()
        .filter(|(_, _, hosts, _)| *hosts == Hosts::Both)
        .map(|(tag, ..)| *tag)
        .collect();
    let excused_web: std::collections::BTreeSet<&str> =
        KNOWN_UNHANDLED.iter().map(|(tag, ..)| *tag).collect();

    let mut missing_viewport: Vec<&String> = Vec::new();
    let mut missing_web: Vec<&String> = Vec::new();
    for tag in &tags {
        if !viewport.contains(tag) && !excused_viewport.contains(tag.as_str()) {
            missing_viewport.push(tag);
        }
        if !web.contains(tag) && !excused_web.contains(tag.as_str()) {
            missing_web.push(tag);
        }
    }

    assert!(
        missing_viewport.is_empty() && missing_web.is_empty(),
        "commands the browser preamble sends that a host drops on the floor.\n\
         The script function will be callable and do nothing — no error, no log.\n\
           editor viewport missing: {missing_viewport:?}\n\
           web runtime missing:     {missing_web:?}\n\
         Add the match arm in `drain_play_commands_into`, or list the tag in \
         KNOWN_UNHANDLED with what it breaks."
    );

    // The other direction: a gap that has been closed must leave the list,
    // or the list rots into fiction and stops meaning anything.
    let stale: Vec<&str> = KNOWN_UNHANDLED
        .iter()
        .filter(|(tag, _, hosts, _)| match hosts {
            Hosts::Both => viewport.contains(*tag) && web.contains(*tag),
            Hosts::Web => web.contains(*tag),
        })
        .map(|(tag, ..)| *tag)
        .collect();
    assert!(
        stale.is_empty(),
        "KNOWN_UNHANDLED lists {stale:?}, which are now handled. Delete them from the list."
    );
}
