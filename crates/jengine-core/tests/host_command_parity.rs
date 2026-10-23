//! Every script command must be handled by every browser-script host.
//!
//! The editor viewport and the web runtime each dispatch the preamble's
//! commands with their own `match` over ~108 tags, hand-copied between two
//! files of 7.3k and 3.3k lines. Adding a script function means editing both;
//! forgetting one is silent, because an unhandled tag is just a command that
//! does nothing in that runtime.
//!
//! This is the single largest source of bugs in the repo (see
//! `docs/architecture-debt.md` #2). Extracting a shared host is the real fix,
//! and 21 world-only commands now live in
//! `jengine_core::scripting::host_commands` — dispatched by both hosts before
//! their own arms. This guards the rest, and additionally guards the shared
//! layer itself: that both hosts still call it, and that neither has grown a
//! private copy of a tag it now handles.
//!
//! The sibling guards check different things and none of them catches this:
//! `script_global_parity` compares *names* between the native registration and
//! the editor allowlist, and `preamble_command_tags` checks each tag has one
//! owner. Neither notices a tag that one host dispatches and the other ignores.

const BROWSER: &str = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
const VIEWPORT: &str = include_str!("../../jengine-editor-viewport/src/lib.rs");
const WEB: &str = include_str!("../../jengine-web/src/script_host.rs");

/// Tags a host may legitimately not dispatch.
///
/// Each entry needs a reason. "It does not work there yet" is a reason; "I
/// forgot" is what this test exists to catch.
const KNOWN_HOST_ONLY: &[(&str, &str)] = &[
    ("lds", "viewport-only: editor-driven scene load, not emitted by the preamble"),
];

/// Tags handled once, in `jengine-core`, rather than by each host.
///
/// A host counts as dispatching these because it calls
/// `handle_world_command` before its own `match` —
/// `both_hosts_call_the_shared_dispatcher` is what makes that assumption safe.
const SHARED: &[&str] = jengine_core::scripting::host_commands::SHARED_WORLD_TAGS;
/// The shared arms that need more than the world (audio, tweens, saves, ...),
/// in `adapters::js::host_dispatch` since Oct 2026.
const HOST_DISPATCH: &[&str] = jengine_core::scripting::host_commands::HOST_DISPATCH_TAGS;
const DISPATCH: &str = include_str!("../src/scripting/bridge/adapters/js/host_dispatch.rs");

/// Extract the tags a `match` arm handles, including alternations like
/// `"a" | "b" => {`.
fn host_arms(src: &str) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for line in src.lines() {
        let t = line.trim();
        if !t.ends_with("=> {") && !t.ends_with("=> {}") && !t.contains("=> ") {
            continue;
        }
        // Only the pattern side, or a string *inside* the arm body would count.
        let Some(pat) = t.split("=>").next() else { continue };
        let pat = pat.trim();
        if !pat.starts_with('"') {
            continue;
        }
        for piece in pat.split('|') {
            let piece = piece.trim();
            if let Some(tag) = piece.strip_prefix('"').and_then(|p| p.strip_suffix('"'))
                && (2..=6).contains(&tag.len())
                && tag.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            {
                out.insert(tag.to_string());
            }
        }
    }
    out
}

/// Tags the browser preamble actually pushes.
fn preamble_tags(src: &str) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    let bytes = src.as_bytes();
    // The key must be exactly `t`, not the tail of a longer one: the preamble
    // also writes `ut: 'f'` for a uniform *type*, and matching that produced
    // phantom tags "f" and "i".
    for (abs, _) in src.match_indices("t: '") {
        if abs > 0 && (bytes[abs - 1] as char).is_ascii_alphanumeric() {
            continue;
        }
        let after = &src[abs + 4..];
        if let Some(end) = after.find('\'') {
            let tag = &after[..end];
            if !tag.is_empty() && tag.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
            {
                out.insert(tag.to_string());
            }
        }
    }
    out
}

#[test]
fn both_hosts_dispatch_the_same_command_tags() {
    let vp = host_arms(VIEWPORT);
    let web = host_arms(WEB);
    let allowed: std::collections::BTreeSet<&str> =
        KNOWN_HOST_ONLY.iter().map(|(t, _)| *t).collect();

    let vp_only: Vec<&String> = vp.difference(&web).filter(|t| !allowed.contains(t.as_str())).collect();
    let web_only: Vec<&String> = web.difference(&vp).filter(|t| !allowed.contains(t.as_str())).collect();

    assert!(
        vp_only.is_empty() && web_only.is_empty(),
        "the two browser-script hosts have drifted.\n  \
         only the editor viewport handles: {vp_only:?}\n  \
         only the web runtime handles:     {web_only:?}\n\
         A command handled by one host and not the other silently does nothing \
         in the other. Add the missing arm, or add the tag to KNOWN_HOST_ONLY \
         with a reason."
    );
}

#[test]
fn every_command_the_preamble_emits_is_handled_by_both_hosts() {
    // The stronger property: it is not enough for the hosts to agree with each
    // other if they both ignore something scripts can call.
    let emitted = preamble_tags(BROWSER);
    assert!(
        emitted.len() > 50,
        "only found {} tags in the preamble — the scraper is broken, not the code",
        emitted.len()
    );

    let allowed: std::collections::BTreeSet<&str> =
        KNOWN_HOST_ONLY.iter().map(|(t, _)| *t).collect();
    for (name, mut arms) in [("viewport", host_arms(VIEWPORT)), ("web", host_arms(WEB))] {
        // A shared tag is dispatched by this host even though the arm is not
        // in this file.
        arms.extend(SHARED.iter().map(|t| t.to_string()));
        let src = if name == "viewport" { VIEWPORT } else { WEB };
        if src.contains("host_dispatch::handle_host_command(") {
            arms.extend(HOST_DISPATCH.iter().map(|t| t.to_string()));
        }
        let missing: Vec<&String> = emitted
            .difference(&arms)
            .filter(|t| !allowed.contains(t.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "{name} does not dispatch {missing:?}, which the preamble emits — \
             scripts calling those functions get silence"
        );
    }
}

#[test]
fn the_known_host_only_list_stays_honest() {
    // An entry that no longer describes reality is worse than no entry: it
    // silences the check for a tag that may since have been implemented, or
    // removed.
    let vp = host_arms(VIEWPORT);
    let web = host_arms(WEB);
    for (tag, reason) in KNOWN_HOST_ONLY {
        assert!(
            vp.contains(*tag) || web.contains(*tag),
            "KNOWN_HOST_ONLY lists `{tag}` ({reason}) but no host handles it — \
             remove the entry"
        );
        assert!(
            !(vp.contains(*tag) && web.contains(*tag)),
            "KNOWN_HOST_ONLY lists `{tag}` ({reason}) but *both* hosts handle it \
             now — remove the exemption so it is guarded like the rest"
        );
    }
}

/// The shared arms only run because each host calls into them. Deleting that
/// one call would silently disable 21 script commands in that runtime, and
/// every other check here would still pass — the tags would simply look
/// "handled" because this file says they are shared.
#[test]
fn both_hosts_call_the_shared_dispatcher() {
    for (name, src) in [("viewport", VIEWPORT), ("web", WEB)] {
        assert!(
            src.contains("host_commands::handle_world_command("),
            "{name} no longer calls the shared command handler, so the {} commands in `scripting::host_commands` do nothing there",
            SHARED.len()
        );
        assert!(
            src.contains("host_dispatch::handle_host_command("),
            "{name} no longer calls `adapters::js::host_dispatch`, so its {} commands do nothing there",
            HOST_DISPATCH.len()
        );
    }
}

/// A host must not keep its own arm for a tag the shared handler claims.
///
/// The shared dispatch runs first, so such an arm would be unreachable — and
/// worse, it would look like the live implementation to anyone reading the
/// host, which is exactly the confusion this extraction exists to remove.
#[test]
fn neither_host_shadows_a_shared_command() {
    let shared: std::collections::BTreeSet<&str> = SHARED.iter().chain(HOST_DISPATCH).copied().collect();
    for (name, arms) in [("viewport", host_arms(VIEWPORT)), ("web", host_arms(WEB))] {
        let shadowed: Vec<&String> =
            arms.iter().filter(|t| shared.contains(t.as_str())).collect();
        assert!(
            shadowed.is_empty(),
            "{name} still has its own arm for {shadowed:?}, which `scripting::host_commands` already handles - the arm is dead code, and a reader would take it for the live implementation"
        );
    }
}

/// Every shared tag must be one the preamble actually emits.
///
/// A tag listed as shared but never emitted is dead weight that also silences
/// the missing-command check above for a name no script can call.
#[test]
fn every_shared_tag_is_one_the_preamble_emits() {
    let emitted = preamble_tags(BROWSER);
    let stray: Vec<&&str> = SHARED.iter().chain(HOST_DISPATCH).filter(|t| !emitted.contains(**t)).collect();
    assert!(
        stray.is_empty(),
        "`SHARED_WORLD_TAGS` lists {stray:?}, which the browser preamble never emits"
    );
}

/// `HOST_DISPATCH_TAGS` is what the hosts and these checks trust: a tag
/// listed but without an arm would vanish, and an arm for a preamble tag the
/// list leaves out would be dispatched while every check said otherwise.
#[test]
fn the_host_dispatch_list_matches_its_arms() {
    let arms = host_arms(DISPATCH);
    let emitted = preamble_tags(BROWSER);
    let listed: std::collections::BTreeSet<&str> = HOST_DISPATCH.iter().copied().collect();
    for tag in HOST_DISPATCH {
        assert!(arms.contains(*tag), "`HOST_DISPATCH_TAGS` lists `{tag}`, which host_dispatch has no arm for");
    }
    for tag in arms.iter().filter(|t| emitted.contains(t.as_str())) {
        assert!(listed.contains(tag.as_str()), "host_dispatch has an arm for `{tag}` that `HOST_DISPATCH_TAGS` leaves out");
    }
}
