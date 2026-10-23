//! Step 3a — script header metadata parser.
//!
//! Scripts may declare runtime hints in a top-level table/object literal:
//!
//! ```lua
//! script = { tick_rate = 10 }
//! ```
//!
//! ```js
//! const script = { tickHz: 10 };
//! // or
//! export const script = { tickHz: 10 };
//! ```
//!
//! For Step 3a we only extract `tick_rate` (Lua) / `tickHz` (JS). The
//! parser is intentionally a regex-style pre-compile scan — no VM
//! round-trip — so it runs once per `load()` call with negligible cost
//! and never fails (an unrecognised header just yields `None`, falling
//! back to the engine default tick rate).
//!
//! Step 3 also extracts the `events` list — the set of well-known
//! lifecycle hooks the script wants delivered (e.g. `on_spawn`,
//! `on_collision_enter`). The runner uses this list at load time to
//! build a subscription index, so dispatch is `O(subscribers)` per
//! event rather than `O(all_loaded)`.
//!
//! ```lua
//! script = { tick_rate = 10, events = { "on_spawn", "on_collision_enter" } }
//! ```
//!
//! ```js
//! const script = { tickHz: 10, events: ["on_spawn", "on_collision_enter"] };
//! ```
//!
//! Future fields (`priority`, `tick_phase = "sync"`, budgets) will be
//! added here as Step 3 grows.

/// Step 3 — well-known engine-emitted lifecycle events that scripts may
/// subscribe to. Stored in a `HashMap` key on the runner's subscription
/// index, so it derives `Hash + Eq`. The order of the variants is the
/// order the events were introduced; do not reorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EngineEvent {
    /// Entity was just loaded into a runner — fired exactly once, right
    /// after the script's `init()` returns successfully.
    Spawn,
    /// Entity is being torn down — fired exactly once, right before the
    /// runner forgets the entity. Receivers MUST NOT rely on the world
    /// still containing the entity (it may already have been removed).
    Despawn,
    /// Two physics bodies started touching this frame. Slice 3.2.
    CollisionEnter,
    /// Two physics bodies stopped touching this frame. Slice 3.2.
    CollisionExit,
    /// A `TriggerArea2D` overlap began or ended this frame. Slice 3.2.
    Trigger,
    /// An `InputActionImpulse` fired (just-pressed action). Slice 3.5.
    Input,
    /// A `Timer` (set via `set_timer`) elapsed. Slice 3.5.
    Timer,
    /// A frame-trigger inside a sprite animation clip fired. Slice 3.5.
    AnimationEvent,
    /// `Visibility` toggled (enabled ↔ disabled). Slice 3.5.
    VisibilityChanged,
    /// A `Health` component crossed a threshold. Slice 3.5.
    HealthChanged,
}

impl EngineEvent {
    /// The script-side function name the adapter dispatches when this
    /// event is delivered. Both Lua and JS use the same names for
    /// portability.
    pub fn handler_name(self) -> &'static str {
        match self {
            EngineEvent::Spawn => "on_spawn",
            EngineEvent::Despawn => "on_despawn",
            EngineEvent::CollisionEnter => "on_collision_enter",
            EngineEvent::CollisionExit => "on_collision_exit",
            EngineEvent::Trigger => "on_trigger",
            EngineEvent::Input => "on_input",
            EngineEvent::Timer => "on_timer",
            EngineEvent::AnimationEvent => "on_animation_event",
            EngineEvent::VisibilityChanged => "on_visibility_changed",
            EngineEvent::HealthChanged => "on_health_changed",
        }
    }

    /// Inverse of [`Self::handler_name`] — recognise an event by its
    /// well-known handler name. Returns `None` for unknown / typo'd
    /// strings; callers (the metadata parser, editor lints) emit a
    /// warning so authors can fix the typo.
    pub fn from_handler_name(name: &str) -> Option<Self> {
        Some(match name {
            "on_spawn" => EngineEvent::Spawn,
            "on_despawn" => EngineEvent::Despawn,
            "on_collision_enter" => EngineEvent::CollisionEnter,
            "on_collision_exit" => EngineEvent::CollisionExit,
            "on_trigger" => EngineEvent::Trigger,
            "on_input" => EngineEvent::Input,
            "on_timer" => EngineEvent::Timer,
            "on_animation_event" => EngineEvent::AnimationEvent,
            "on_visibility_changed" => EngineEvent::VisibilityChanged,
            "on_health_changed" => EngineEvent::HealthChanged,
            _ => return None,
        })
    }
}

/// Parse the declared `tick_rate` / `tickHz` value from a Lua or JS
/// source file's top-level `script = { ... }` literal.
///
/// Returns `None` when no such literal is present, when the field is
/// missing, when the value is not a positive integer, or when the value
/// is `> u32::MAX / 2` (sanity bound — anything beyond a few hundred Hz
/// degrades to "every frame" anyway).
///
/// The scan is whitespace-tolerant and accepts the Lua and JS forms in
/// the same pass:
///
/// * Lua: `script = { tick_rate = 10 }` (key: `tick_rate`, equals
///   separator).
/// * JS: `script = { tickHz: 10 }` or `const script = { tickHz: 10 };` or
///   `export const script = { tickHz: 10 };` (key: `tickHz`, colon
///   separator).
///
/// Additional whitespace, line breaks inside the table, and a trailing
/// comma are all permitted. The first match wins; sources that declare
/// multiple `script = {...}` blocks are not supported (no real script
/// would).
pub fn parse_tick_rate(source: &str) -> Option<u32> {
    // Locate `script` followed by `=` and an opening brace `{`.
    let body = locate_script_table_body(source)?;
    // Within the table body, look for either Lua key/value or JS
    // key/value spellings of the tick rate field.
    parse_kv_u32(body, "tick_rate", '=').or_else(|| parse_kv_u32(body, "tickHz", ':'))
}

/// Phase B — parse the `extends = "lib/parent"` field from the same
/// `script = { ... }` header. Returns the parent module path verbatim
/// (no canonicalisation). The value must be a quoted string literal —
/// either `"..."` or `'...'`. Whitespace around the `=`/`:` separator
/// and inside the table is tolerated. Returns `None` when the field is
/// absent, the value isn't a quoted string, or no `script` table is
/// present at all.
///
/// Recognises both Lua (`extends = "lib/foo"`) and JS
/// (`extends: "lib/foo"`) spellings in the same pass — the resolver
/// owning the path interprets it (typically a path under
/// `assets/scripts/`).
pub fn parse_extends(source: &str) -> Option<String> {
    let body = locate_script_table_body(source)?;
    parse_kv_string(body, "extends")
}

/// Step 3 — parse the `events` list from the same `script = { ... }`
/// header. Returns the deduplicated set of well-known [`EngineEvent`]s
/// the script wants delivered, in declaration order.
///
/// Recognises both the Lua list form (`events = { "on_spawn", ... }`)
/// and the JS array form (`events: ["on_spawn", ...]`). Whitespace is
/// tolerated between every token. Single quotes are accepted as well
/// as double quotes (matches both languages' string literal syntax).
///
/// Unknown handler names (typos like `on_spwan`) are silently dropped —
/// callers that want to surface them as warnings (editor lints) can
/// re-parse with a stricter helper. Returning the silent-drop here keeps
/// runtime behaviour identical between authoring environments where the
/// editor lint runs and command-line builds where it doesn't.
///
/// Returns an empty `Vec` when no `events` field is present, which is
/// the common case (most scripts don't subscribe to any lifecycle
/// events). Callers should treat empty and missing identically.
pub fn parse_subscribed_events(source: &str) -> Vec<EngineEvent> {
    let Some(body) = locate_script_table_body(source) else {
        return Vec::new();
    };
    // Look for `events` followed by `=` (Lua) or `:` (JS) and an
    // opening list/array delimiter (`{` for Lua, `[` for JS).
    let Some(list_body) = parse_kv_list(body, "events") else {
        return Vec::new();
    };
    let mut out: Vec<EngineEvent> = Vec::new();
    for raw in split_quoted_strings(list_body) {
        if let Some(ev) = EngineEvent::from_handler_name(raw)
            && !out.contains(&ev) {
                out.push(ev);
            }
    }
    out
}

/// Locate `<key>` in `body` followed by `=` or `:` and an opening `{`
/// or `[`, then walk to the matching closing delimiter and return the
/// inner slice. Used by [`parse_subscribed_events`] to find the
/// `events = {...}` / `events: [...]` body.
fn parse_kv_list<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    let bytes = body.as_bytes();
    let key_bytes = key.as_bytes();
    let klen = key_bytes.len();
    if klen == 0 || bytes.len() < klen {
        return None;
    }
    let mut i = 0;
    while i + klen <= bytes.len() {
        if &bytes[i..i + klen] == key_bytes {
            let prev_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let next_ok = i + klen == bytes.len() || !is_ident_byte(bytes[i + klen]);
            if prev_ok && next_ok {
                let mut j = i + klen;
                j = skip_ws(bytes, j);
                if j < bytes.len() && (bytes[j] == b'=' || bytes[j] == b':') {
                    j += 1;
                    j = skip_ws(bytes, j);
                    if j < bytes.len() && (bytes[j] == b'{' || bytes[j] == b'[') {
                        let open = bytes[j];
                        let close = if open == b'{' { b'}' } else { b']' };
                        let body_start = j + 1;
                        let mut depth: u32 = 1;
                        let mut k = body_start;
                        while k < bytes.len() {
                            if bytes[k] == open {
                                depth += 1;
                            } else if bytes[k] == close {
                                depth -= 1;
                                if depth == 0 {
                                    return Some(&body[body_start..k]);
                                }
                            }
                            k += 1;
                        }
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// Yield every quoted string literal inside `body`, stripping the
/// surrounding quote pair. Accepts both `"..."` and `'...'`. Does NOT
/// support escape sequences — handler names are plain ASCII identifiers
/// so this is fine, and avoiding escape parsing keeps the scan
/// allocation-free until the actual `&str` slices are returned.
fn split_quoted_strings(body: &str) -> Vec<&str> {
    let bytes = body.as_bytes();
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' || c == b'\'' {
            let quote = c;
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != quote {
                j += 1;
            }
            if j <= bytes.len() && j >= start {
                out.push(&body[start..j]);
            }
            i = j + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// Scan `source` for a top-level `script` identifier whose right-hand
/// side is an object/table literal. Returns the substring between the
/// matched `{` and its matching `}`, or `None` when no such literal is
/// found. Top-level matches only — a `script` field nested inside a
/// function body is ignored because the scan starts from byte 0 and
/// finds the *first* match (real script files declare metadata at the
/// very top, by convention).
///
/// Comments are skipped: a file whose opening comment *describes* the
/// header - `-- script inheritance, script = { extends = "..." }` - used to
/// be read as declaring it, and then failed to `require('...')` (scene 103's
/// own controller, in every runtime; the editor's `scriptTableBody` is the
/// same rule and skips them too).
fn locate_script_table_body(source: &str) -> Option<&str> {
    // Find the first occurrence of the bare identifier `script` that is
    // followed (with optional whitespace) by `=` and a `{`. We skip
    // matches where the preceding character makes `script` part of a
    // larger identifier (`my_script`, `scripted`, etc.).
    let bytes = source.as_bytes();
    let mut i = 0;
    while i + 6 <= bytes.len() {
        if let Some(end) = comment_end(bytes, i) {
            i = end;
            continue;
        }
        if &bytes[i..i + 6] == b"script" {
            let prev_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let after_ok = i + 6 == bytes.len() || !is_ident_byte(bytes[i + 6]);
            if prev_ok && after_ok
                && let Some(body) = try_extract_table_after(source, i + 6) {
                    return Some(body);
                }
        }
        i += 1;
    }
    None
}

/// Starting just after the `script` identifier, skip whitespace and an
/// optional `=`, then if the next non-whitespace char is `{` walk
/// brace-balanced to the matching close brace and return the inner
/// slice. Returns `None` on any deviation from that shape.
fn try_extract_table_after(source: &str, start: usize) -> Option<&str> {
    let bytes = source.as_bytes();
    let mut j = start;
    j = skip_ws(bytes, j);
    // Tolerate the `=` (used by both Lua and JS top-level assignment).
    if j < bytes.len() && bytes[j] == b'=' {
        j += 1;
        j = skip_ws(bytes, j);
    }
    if j >= bytes.len() || bytes[j] != b'{' {
        return None;
    }
    let body_start = j + 1;
    // Brace-balanced walk. Step 3a only looks for a top-level numeric
    // value — strings/comments inside the table body would only matter
    // for a richer parser, and parse_kv_u32 already requires the value
    // to be a digit run.
    let mut depth: u32 = 1;
    let mut k = body_start;
    while k < bytes.len() {
        match bytes[k] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&source[body_start..k]);
                }
            }
            _ => {}
        }
        k += 1;
    }
    None
}

/// Look for `<key><sep><digits>` within `body`, with the key bounded by
/// non-identifier bytes on the left so `notick_rate` doesn't match. Any
/// amount of whitespace is allowed around the separator. Returns the
/// parsed number, or `None` if the key is absent or the value is not a
/// pure positive integer.
fn parse_kv_u32(body: &str, key: &str, sep: char) -> Option<u32> {
    let bytes = body.as_bytes();
    let key_bytes = key.as_bytes();
    let klen = key_bytes.len();
    if klen == 0 || bytes.len() < klen {
        return None;
    }
    let mut i = 0;
    while i + klen <= bytes.len() {
        if &bytes[i..i + klen] == key_bytes {
            let prev_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let next_ok = i + klen == bytes.len() || !is_ident_byte(bytes[i + klen]);
            if prev_ok && next_ok {
                let mut j = i + klen;
                j = skip_ws(bytes, j);
                if j < bytes.len() && bytes[j] == sep as u8 {
                    j += 1;
                    j = skip_ws(bytes, j);
                    let start = j;
                    while j < bytes.len() && bytes[j].is_ascii_digit() {
                        j += 1;
                    }
                    if j > start {
                        // Reject `10.5` / `10e3` / `10n` — only a clean
                        // integer terminator (whitespace, comma,
                        // semicolon, close-brace, or EOF) is accepted
                        // so a fractional value falls back to the
                        // engine default rather than silently
                        // truncating.
                        let terminator_ok = j == bytes.len()
                            || matches!(bytes[j], b' ' | b'\t' | b'\r' | b'\n' | b',' | b';' | b'}');
                        if !terminator_ok {
                            i += 1;
                            continue;
                        }
                        if let Ok(n) = body[start..j].parse::<u32>() {
                            // Guard against silly large values; treat
                            // `0` as "fall back to default" so a
                            // `tick_rate = 0` typo doesn't disable
                            // updates entirely.
                            if n > 0 && n < u32::MAX / 2 {
                                return Some(n);
                            }
                        }
                    }
                }
            }
        }
        i += 1;
    }
    None
}

#[inline]
/// If a comment starts at `i`, the index just past it: Lua's `--` and
/// `--[[ ]]` (with any number of `=`), JS's `//` and `/* */`. A `//` in Luau
/// is also floor division, which this would read as a comment - harmless
/// here, since a header does not share a line with arithmetic.
fn comment_end(bytes: &[u8], i: usize) -> Option<usize> {
    let rest = &bytes[i..];
    let to_line_end = || bytes[i..].iter().position(|&b| b == b'\n').map_or(bytes.len(), |n| i + n);
    if rest.starts_with(b"--") {
        // A long comment: `--[`, some `=`, `[`.
        let mut j = 2;
        if rest.get(j) == Some(&b'[') {
            j += 1;
            let level = rest[j..].iter().take_while(|&&b| b == b'=').count();
            j += level;
            if rest.get(j) == Some(&b'[') {
                let mut close = vec![b']'];
                close.extend(std::iter::repeat_n(b'=', level));
                close.push(b']');
                let body = &rest[j + 1..];
                return Some(
                    body.windows(close.len())
                        .position(|w| w == close.as_slice())
                        .map_or(bytes.len(), |n| i + j + 1 + n + close.len()),
                );
            }
        }
        return Some(to_line_end());
    }
    if rest.starts_with(b"//") {
        return Some(to_line_end());
    }
    if rest.starts_with(b"/*") {
        return Some(rest[2..].windows(2).position(|w| w == b"*/").map_or(bytes.len(), |n| i + 2 + n + 2));
    }
    None
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

#[inline]
fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\r' | b'\n' => i += 1,
            _ => break,
        }
    }
    i
}

/// Phase B helper — look for `<key>` followed by `=`/`:` and a quoted
/// string literal (single or double quote). Returns the unescaped string
/// content. No backslash escape handling — paths are plain and won't
/// contain quotes.
fn parse_kv_string(body: &str, key: &str) -> Option<String> {
    let bytes = body.as_bytes();
    let key_bytes = key.as_bytes();
    let klen = key_bytes.len();
    if klen == 0 || bytes.len() < klen {
        return None;
    }
    let mut i = 0;
    while i + klen <= bytes.len() {
        if &bytes[i..i + klen] == key_bytes {
            let prev_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let next_ok = i + klen == bytes.len() || !is_ident_byte(bytes[i + klen]);
            if prev_ok && next_ok {
                let mut j = i + klen;
                j = skip_ws(bytes, j);
                if j < bytes.len() && (bytes[j] == b'=' || bytes[j] == b':') {
                    j += 1;
                    j = skip_ws(bytes, j);
                    if j < bytes.len() && (bytes[j] == b'"' || bytes[j] == b'\'') {
                        let quote = bytes[j];
                        let start = j + 1;
                        let mut k = start;
                        while k < bytes.len() && bytes[k] != quote {
                            k += 1;
                        }
                        if k < bytes.len() {
                            return Some(body[start..k].to_string());
                        }
                    }
                }
            }
        }
        i += 1;
    }
    None
}

/// Strip the `script = { ... }` metadata block from Lua source before
/// transpilation.
///
/// When the Lua→JS transpiler encounters `script = { tick_rate = 10 }` it
/// emits `var script = { ... };`, which conflicts with names in the JS
/// sandbox wrapper. This function removes the entire statement — from the
/// `script` keyword through the matching `}`, plus any trailing semicolon
/// and the newline — so the transpiled output contains only executable
/// game logic.
///
/// The header may span multiple lines:
/// ```text
/// script = {
///     tick_rate = 10,
///     events = { "on_spawn" },
/// }
/// ```
/// All of those lines are removed. Returns the source unchanged when no
/// matching header is found.
pub fn strip_script_header(source: &str) -> String {
    /// `true` if `prefix` contains only whitespace and Lua/JS line comments
    /// (`-- ...` or `// ...` to end-of-line). Used to confirm the candidate
    /// `script = { ... }` is the first non-trivial statement in the file
    /// (and therefore a real header, not an indented assignment inside a
    /// function body).
    fn is_only_trivia_prefix(prefix: &str) -> bool {
        let bytes = prefix.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b' ' || b == b'\t' || b == b'\r' || b == b'\n' {
                i += 1;
                continue;
            }
            if b == b'-' && i + 1 < bytes.len() && bytes[i + 1] == b'-' {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            if b == b'/' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                i += 2;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            return false;
        }
        true
    }

    let bytes = source.as_bytes();
    let mut i = 0;
    while i + 6 <= bytes.len() {
        if &bytes[i..i + 6] == b"script" {
            let prev_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
            let after_ok = i + 6 == bytes.len() || !is_ident_byte(bytes[i + 6]);
            if prev_ok && after_ok {
                let mut j = i + 6;
                j = skip_ws(bytes, j);
                if j < bytes.len() && bytes[j] == b'=' {
                    j += 1;
                    j = skip_ws(bytes, j);
                    if j < bytes.len() && bytes[j] == b'{' {
                        // Walk brace-balanced to the closing `}`.
                        let mut depth: u32 = 1;
                        let mut k = j + 1;
                        while k < bytes.len() {
                            match bytes[k] {
                                b'{' => depth += 1,
                                b'}' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        // Verify the `script` token is the
                                        // first non-trivial statement in the
                                        // file — i.e. everything between the
                                        // source start and `i` must be only
                                        // whitespace, blank lines, or Lua/JS
                                        // line comments. This prevents stripping
                                        // an `script = { ... }` assignment that
                                        // appears inside a function body.
                                        let line_start = source[..i]
                                            .rfind('\n')
                                            .map(|p| p + 1)
                                            .unwrap_or(0);
                                        if is_only_trivia_prefix(&source[..i]) {
                                            // Skip optional trailing semicolon.
                                            let mut end = k + 1;
                                            if end < bytes.len() && bytes[end] == b';' {
                                                end += 1;
                                            }
                                            // Skip the trailing newline (CRLF or LF).
                                            if end < bytes.len() && bytes[end] == b'\r' {
                                                end += 1;
                                            }
                                            if end < bytes.len() && bytes[end] == b'\n' {
                                                end += 1;
                                            }
                                            return format!(
                                                "{}{}",
                                                &source[..line_start],
                                                &source[end..]
                                            );
                                        }
                                        break;
                                    }
                                }
                                _ => {}
                            }
                            k += 1;
                        }
                    }
                }
            }
        }
        i += 1;
    }
    source.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_header_in_a_comment_is_not_a_header() {
        // Scene 103's controller: its first line describes `extends`.
        let src = "-- Scene 103: `script = { extends = \"...\" }` and `super`.\nlocal nav = require(\"lua/nav\")\n";
        assert_eq!(parse_extends(src), None);
        let block = "--[[ script = { extends = \"a\" } ]]\n--[==[ script = { extends = \"b\" } ]==]\n/* script = { extends: \"c\" } */\n// script = { extends: \"d\" }\n";
        assert_eq!(parse_extends(block), None);
        // A real header after a comment that mentions one still counts.
        let real = "-- like script = { extends = \"no\" }\nscript = { extends = \"lib/yes\" }\n";
        assert_eq!(parse_extends(real).as_deref(), Some("lib/yes"));
        let after_block = "--[[ script = { tick_rate = 1 } ]]\nscript = { tick_rate = 7 }\n";
        assert_eq!(parse_tick_rate(after_block), Some(7));
    }

    #[test]
    fn parses_lua_table_form() {
        let src = "script = { tick_rate = 10 }\nfunction update(e, dt) end\n";
        assert_eq!(parse_tick_rate(src), Some(10));
    }

    #[test]
    fn parses_lua_table_form_multiline() {
        let src = "script = {\n    tick_rate = 5,\n}\n";
        assert_eq!(parse_tick_rate(src), Some(5));
    }

    #[test]
    fn parses_js_const_form() {
        let src = "const script = { tickHz: 30 };\n";
        assert_eq!(parse_tick_rate(src), Some(30));
    }

    #[test]
    fn parses_js_export_const_form() {
        let src = "export const script = { tickHz: 15 };\n";
        assert_eq!(parse_tick_rate(src), Some(15));
    }

    #[test]
    fn parses_js_bare_form() {
        let src = "script = { tickHz: 20 };\n";
        assert_eq!(parse_tick_rate(src), Some(20));
    }

    #[test]
    fn missing_metadata_returns_none() {
        let src = "function update(e, dt) end\n";
        assert_eq!(parse_tick_rate(src), None);
    }

    #[test]
    fn wrong_key_returns_none() {
        let src = "script = { tick = 10 }\n";
        assert_eq!(parse_tick_rate(src), None);
    }

    #[test]
    fn zero_returns_none() {
        // tick_rate = 0 is a typo; fall back to engine default rather
        // than disable updates entirely.
        let src = "script = { tick_rate = 0 }\n";
        assert_eq!(parse_tick_rate(src), None);
    }

    #[test]
    fn ignores_substring_identifiers() {
        // `myscript = ...` and `tickHz_extra` must not match.
        let src = "myscript = { tickHz_extra: 99 }\n";
        assert_eq!(parse_tick_rate(src), None);
    }

    #[test]
    fn ignores_no_table_form() {
        // Bare `script = 10` is not a table literal — Step 3a only
        // recognises the table form so future fields can be added
        // without re-parsing.
        let src = "script = 10\n";
        assert_eq!(parse_tick_rate(src), None);
    }

    #[test]
    fn extra_whitespace_tolerated() {
        let src = "  script   =   {   tick_rate   =   12   }\n";
        assert_eq!(parse_tick_rate(src), Some(12));
    }

    #[test]
    fn trailing_fields_ignored() {
        // Step 3a only reads tick_rate / tickHz; future fields like
        // `events = {...}` must not throw the parser off.
        let src = "script = { tick_rate = 8, events = { 'foo' } }\n";
        assert_eq!(parse_tick_rate(src), Some(8));
    }

    #[test]
    fn non_integer_value_returns_none() {
        let src = "script = { tick_rate = 10.5 }\n";
        assert_eq!(parse_tick_rate(src), None);
    }

    #[test]
    fn js_form_takes_precedence_when_both_present() {
        // Defensive: if a transpiled script ever ends up with both
        // forms, we just want a deterministic answer. Lua key wins
        // because `parse_tick_rate` checks it first; record this so
        // the behaviour is intentional.
        let src = "script = { tick_rate = 4, tickHz: 8 }\n";
        assert_eq!(parse_tick_rate(src), Some(4));
    }

    // ---------------- parse_subscribed_events ----------------

    #[test]
    fn no_events_field_returns_empty() {
        let src = "script = { tick_rate = 10 }\n";
        assert!(parse_subscribed_events(src).is_empty());
    }

    #[test]
    fn missing_metadata_returns_empty_events() {
        let src = "function update(e, dt) end\n";
        assert!(parse_subscribed_events(src).is_empty());
    }

    #[test]
    fn parses_lua_events_list() {
        let src =
            "script = { tick_rate = 10, events = { \"on_spawn\", \"on_collision_enter\" } }\n";
        let evs = parse_subscribed_events(src);
        assert_eq!(evs, vec![EngineEvent::Spawn, EngineEvent::CollisionEnter]);
    }

    #[test]
    fn parses_js_events_array() {
        let src = "const script = { tickHz: 10, events: [\"on_spawn\", \"on_despawn\"] };\n";
        let evs = parse_subscribed_events(src);
        assert_eq!(evs, vec![EngineEvent::Spawn, EngineEvent::Despawn]);
    }

    #[test]
    fn parses_single_quoted_strings() {
        let src = "script = { events = { 'on_spawn', 'on_trigger' } }\n";
        let evs = parse_subscribed_events(src);
        assert_eq!(evs, vec![EngineEvent::Spawn, EngineEvent::Trigger]);
    }

    #[test]
    fn unknown_handler_names_are_dropped_silently() {
        // Typo: `on_spwan` is not a real event. Drop it; keep the
        // good one. Matches `from_handler_name` returning None.
        let src = "script = { events = { \"on_spwan\", \"on_spawn\" } }\n";
        let evs = parse_subscribed_events(src);
        assert_eq!(evs, vec![EngineEvent::Spawn]);
    }

    #[test]
    fn duplicates_are_deduplicated_in_declaration_order() {
        let src = "script = { events = { \"on_spawn\", \"on_input\", \"on_spawn\" } }\n";
        let evs = parse_subscribed_events(src);
        assert_eq!(evs, vec![EngineEvent::Spawn, EngineEvent::Input]);
    }

    #[test]
    fn events_field_only_returns_events() {
        // `events` without a `tick_rate` field should still parse.
        let src = "script = { events = { \"on_spawn\" } }\n";
        let evs = parse_subscribed_events(src);
        assert_eq!(evs, vec![EngineEvent::Spawn]);
    }

    #[test]
    fn empty_events_list_returns_empty() {
        let src = "script = { events = {} }\n";
        assert!(parse_subscribed_events(src).is_empty());
    }

    // ---------------- parse_extends ----------------

    #[test]
    fn parses_lua_extends_double_quoted() {
        let src = "script = { extends = \"lib/creature\" }\n";
        assert_eq!(parse_extends(src).as_deref(), Some("lib/creature"));
    }

    #[test]
    fn parses_lua_extends_single_quoted() {
        let src = "script = { extends = 'lib/creature' }\n";
        assert_eq!(parse_extends(src).as_deref(), Some("lib/creature"));
    }

    #[test]
    fn parses_js_extends_form() {
        let src = "const script = { tickHz: 10, extends: \"lib/foe\" };\n";
        assert_eq!(parse_extends(src).as_deref(), Some("lib/foe"));
    }

    #[test]
    fn parses_extends_alongside_other_fields() {
        let src = "script = { tick_rate = 5, extends = \"lib/npc\", events = { \"on_spawn\" } }\n";
        assert_eq!(parse_extends(src).as_deref(), Some("lib/npc"));
    }

    #[test]
    fn missing_extends_returns_none() {
        let src = "script = { tick_rate = 10 }\n";
        assert_eq!(parse_extends(src), None);
    }

    #[test]
    fn no_script_table_returns_none_for_extends() {
        let src = "function update() end\n";
        assert_eq!(parse_extends(src), None);
    }

    #[test]
    fn extends_unquoted_value_returns_none() {
        // Identifiers (e.g. `extends = lib_creature`) are not supported —
        // the resolver expects a literal path.
        let src = "script = { extends = lib_creature }\n";
        assert_eq!(parse_extends(src), None);
    }

    // ---------------- strip_script_header ----------------

    #[test]
    fn strips_single_line_lua_header() {
        let src = "script = { tick_rate = 10 }\nfunction update(e, dt) end\n";
        assert_eq!(
            strip_script_header(src),
            "function update(e, dt) end\n"
        );
    }

    #[test]
    fn strips_multiline_lua_header() {
        let src = "script = {\n    tick_rate = 10,\n    events = { \"on_spawn\" },\n}\nfunction update(e, dt) end\n";
        assert_eq!(strip_script_header(src), "function update(e, dt) end\n");
    }

    #[test]
    fn strips_header_with_trailing_semicolon() {
        let src = "script = { tick_rate = 5 };\nfunction update(e, dt) end\n";
        assert_eq!(
            strip_script_header(src),
            "function update(e, dt) end\n"
        );
    }

    #[test]
    fn no_header_returns_unchanged() {
        let src = "function update(e, dt) end\n";
        assert_eq!(strip_script_header(src), src);
    }

    #[test]
    fn does_not_strip_nested_script_table() {
        // `script` inside a function body (not top-level) must not be stripped.
        let src = "function setup()\n  script = { tick_rate = 10 }\nend\n";
        // The `script =` line has leading whitespace/indent — not top-level.
        assert_eq!(strip_script_header(src), src);
    }

    #[test]
    fn handler_name_round_trips() {
        let all = [
            EngineEvent::Spawn,
            EngineEvent::Despawn,
            EngineEvent::CollisionEnter,
            EngineEvent::CollisionExit,
            EngineEvent::Trigger,
            EngineEvent::Input,
            EngineEvent::Timer,
            EngineEvent::AnimationEvent,
            EngineEvent::VisibilityChanged,
            EngineEvent::HealthChanged,
        ];
        for ev in all {
            assert_eq!(
                EngineEvent::from_handler_name(ev.handler_name()),
                Some(ev),
                "round-trip failed for {:?}",
                ev
            );
        }
    }
}
