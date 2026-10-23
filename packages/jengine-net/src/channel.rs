//! The script-facing side of the connection, carried on a custom component.
//!
//! **Why not script globals.** A package can register real script functions,
//! but `install_script_api` is only wired into `ScriptSystem` — the native
//! Luau and QuickJS bridges. The editor viewport and the web runtime execute
//! Lua through Fengari with a hardcoded `BRIDGE_FUNCTIONS` allowlist that
//! knows nothing about packages, so a `net_send()` global would be `nil` in
//! the runtime you actually develop in. Custom components have no such
//! split: `get_custom` / `set_custom` are core bridge functions present
//! everywhere, so this channel behaves the same in all four.
//!
//! The shape, on any entity carrying a `"Net"` custom component:
//!
//! | Field | Written by | Meaning |
//! |---|---|---|
//! | `url` | script | server to be connected to; empty means disconnect |
//! | `outbox` | script | array of strings to send; the package clears it |
//! | `inbox` | package | array of received strings; the script clears it |
//! | `state` | package | `"disconnected"` … `"failed"` |
//! | `dropped` | package | messages lost to a full inbox this session |
//! | `requests` | script | HTTP requests to make; the package clears them |
//! | `responses` | package | HTTP results; the script clears them |
//!
//! ```lua
//! local net = find_by_name("Net")
//! -- send
//! local out = get_custom(net, "Net", "outbox") or {}
//! table.insert(out, '{"type":"move","x":3}')
//! set_custom(net, "Net", "outbox", out)
//! -- receive
//! for _, msg in ipairs(get_custom(net, "Net", "inbox") or {}) do handle(msg) end
//! set_custom(net, "Net", "inbox", {})
//! ```
//!
//! Binary frames are deliberately absent here: there is no clean way to carry
//! bytes through a Lua table without inventing an encoding, and a text channel
//! is enough for JSON protocols. Rust systems keep full access to both.

/// Custom-component key this channel lives under.
pub const COMPONENT: &str = "Net";

/// Field names. Constants rather than literals so a rename is one edit and
/// the tests pin the contract scripts depend on.
pub const FIELD_URL: &str = "url";
pub const FIELD_OUTBOX: &str = "outbox";
pub const FIELD_INBOX: &str = "inbox";
pub const FIELD_STATE: &str = "state";
pub const FIELD_DROPPED: &str = "dropped";
pub const FIELD_REQUESTS: &str = "requests";
pub const FIELD_RESPONSES: &str = "responses";
/// `true` on a `Net` component that wants a **connection of its own** rather
/// than the one every `Net` component shares: a scene with two sessions (a
/// split screen, two servers). Set by `jengine-online` on its entity.
pub const FIELD_OWN: &str = "own";

/// Cap on how many inbound messages are mirrored into a script inbox per
/// frame.
///
/// The Rust-side inbox is already bounded; this is a second, tighter bound
/// because every message here is serialized into a `serde_json::Value` that a
/// script will walk. A flood should cost a bounded amount of per-frame work
/// rather than however much the socket managed to deliver.
pub const MAX_INBOX_PER_FRAME: usize = 256;

/// The server a script is asking to be connected to.
///
/// Declarative on purpose: a script states the URL it wants and the package
/// reconciles. The alternative - `net_connect()` / `net_disconnect()` calls -
/// makes the script responsible for remembering whether it already issued one,
/// which goes wrong the moment a scene reloads and the script runs `init`
/// again against a connection that is still open.
///
/// `None` when the field is absent or not a string; empty string means
/// "disconnect", which is distinct from absent.
pub fn requested_url(component: &serde_json::Value) -> Option<&str> {
    component.get(FIELD_URL)?.as_str()
}

/// Whether this component asked for a connection of its own ([`FIELD_OWN`]).
pub fn owns_connection(component: &serde_json::Value) -> bool {
    component.get(FIELD_OWN).and_then(|v| v.as_bool()).unwrap_or(false)
}

/// Take the HTTP requests a script queued, leaving the field empty.
///
/// Each entry is an object: `{ id, url, method?, headers?, body? }`. `method`
/// defaults to GET. An entry missing a `url`, or naming a method that is not
/// a real one, is reported and skipped rather than guessed at — a typo that
/// silently became a GET would fail at the server for reasons the caller then
/// has to work backwards from.
pub fn take_requests(component: &mut serde_json::Value) -> Vec<crate::http::HttpRequest> {
    use crate::http::{HttpMethod, HttpRequest};

    let Some(obj) = component.as_object_mut() else {
        return Vec::new();
    };
    let Some(field) = obj.get_mut(FIELD_REQUESTS) else {
        return Vec::new();
    };
    let Some(items) = field.as_array_mut() else {
        log::warn!("[net] {COMPONENT}.{FIELD_REQUESTS} is not an array; ignoring");
        *field = serde_json::Value::Array(Vec::new());
        return Vec::new();
    };

    let mut out = Vec::new();
    for item in items.iter() {
        let Some(url) = item.get("url").and_then(|v| v.as_str()) else {
            log::warn!("[net] an HTTP request entry has no `url`; skipping");
            continue;
        };
        // The id is how a script pairs a response with what it asked for.
        // Defaulting it to the URL is better than an empty string, which
        // would make every anonymous request indistinguishable.
        let id = item
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or(url)
            .to_string();

        let method = match item.get("method").and_then(|v| v.as_str()) {
            None => HttpMethod::Get,
            Some(name) => match HttpMethod::parse(name) {
                Some(m) => m,
                None => {
                    log::warn!("[net] '{name}' is not an HTTP method; skipping '{id}'");
                    continue;
                }
            },
        };

        let headers = item
            .get("headers")
            .and_then(|v| v.as_object())
            .map(|map| {
                map.iter()
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
                    .collect()
            })
            .unwrap_or_default();

        // A non-string body is almost always a table the script meant to
        // encode; sending its debug form would be a server-side mystery.
        let body = match item.get("body") {
            None | Some(serde_json::Value::Null) => None,
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(_) => {
                log::warn!("[net] request '{id}' has a non-string body; encode it first");
                continue;
            }
        };

        out.push(HttpRequest { id, method, url: url.to_string(), headers, body });
    }
    items.clear();
    out
}

/// Append HTTP results for a script to read.
pub fn publish_responses(
    component: &mut serde_json::Value,
    responses: &[crate::http::HttpResponse],
) {
    if responses.is_empty() {
        return;
    }
    if !component.is_object() {
        *component = serde_json::json!({});
    }
    let obj = component.as_object_mut().expect("just made it an object");
    let field = obj
        .entry(FIELD_RESPONSES.to_string())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    if !field.is_array() {
        *field = serde_json::Value::Array(Vec::new());
    }
    let items = field.as_array_mut().expect("just made it an array");
    for r in responses {
        if items.len() >= MAX_INBOX_PER_FRAME {
            log::warn!("[net] HTTP response list is full; is the script clearing it?");
            break;
        }
        items.push(serde_json::json!({
            "id": r.id,
            "status": r.status,
            "ok": r.ok,
            "body": r.body,
            "error": r.error,
        }));
    }
}

/// Take the strings a script queued, leaving the outbox empty.
///
/// Non-string entries are skipped with a warning rather than stringified: a
/// script that put a table there meant to encode it first, and silently
/// sending `[object]` would be a bug that only shows up server-side.
pub fn take_outbox(component: &mut serde_json::Value) -> Vec<String> {
    let Some(obj) = component.as_object_mut() else {
        return Vec::new();
    };
    let Some(outbox) = obj.get_mut(FIELD_OUTBOX) else {
        return Vec::new();
    };
    let Some(items) = outbox.as_array_mut() else {
        // A script that set `outbox` to something other than an array gets
        // told once, and the field is reset so it stops being wrong forever.
        log::warn!("[net] {COMPONENT}.{FIELD_OUTBOX} is not an array; ignoring");
        *outbox = serde_json::Value::Array(Vec::new());
        return Vec::new();
    };

    let mut out = Vec::with_capacity(items.len());
    for item in items.iter() {
        match item.as_str() {
            Some(s) => out.push(s.to_string()),
            None => log::warn!(
                "[net] {COMPONENT}.{FIELD_OUTBOX} entry is not a string; encode it first"
            ),
        }
    }
    items.clear();
    out
}

/// Append received messages to the script inbox, and publish the state.
///
/// Appends rather than replaces so a script that reads once every few frames
/// does not miss traffic — at the cost of needing to clear the field, which
/// the module docs spell out.
pub fn publish(
    component: &mut serde_json::Value,
    incoming: &[String],
    state: &str,
    dropped: u64,
) {
    if !component.is_object() {
        *component = serde_json::json!({});
    }
    let obj = component.as_object_mut().expect("just made it an object");

    obj.insert(FIELD_STATE.to_string(), serde_json::Value::String(state.to_string()));
    obj.insert(FIELD_DROPPED.to_string(), serde_json::Value::from(dropped));

    if incoming.is_empty() {
        return;
    }
    let inbox = obj
        .entry(FIELD_INBOX.to_string())
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    if !inbox.is_array() {
        *inbox = serde_json::Value::Array(Vec::new());
    }
    let items = inbox.as_array_mut().expect("just made it an array");
    for msg in incoming {
        if items.len() >= MAX_INBOX_PER_FRAME {
            log::warn!(
                "[net] script inbox at {MAX_INBOX_PER_FRAME} messages; \
                 dropping the rest this frame - is the script clearing it?"
            );
            break;
        }
        items.push(serde_json::Value::String(msg.clone()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn take_outbox_returns_strings_and_empties_the_field() {
        let mut c = json!({ "outbox": ["a", "b"] });
        assert_eq!(take_outbox(&mut c), vec!["a".to_string(), "b".to_string()]);
        assert_eq!(c["outbox"], json!([]), "the package owns clearing it");
    }

    #[test]
    fn a_non_string_entry_is_skipped_not_stringified() {
        // Sending `[object Object]` to a server is a bug that only shows up
        // on the far side; dropping it locally is the loud failure.
        let mut c = json!({ "outbox": ["ok", { "x": 1 }, 42] });
        assert_eq!(take_outbox(&mut c), vec!["ok".to_string()]);
    }

    #[test]
    fn an_outbox_of_the_wrong_type_is_reset() {
        let mut c = json!({ "outbox": "not an array" });
        assert!(take_outbox(&mut c).is_empty());
        assert_eq!(c["outbox"], json!([]), "reset so it stops being wrong every frame");
    }

    #[test]
    fn a_missing_outbox_is_not_an_error() {
        let mut c = json!({});
        assert!(take_outbox(&mut c).is_empty());
    }

    #[test]
    fn publish_writes_state_and_appends_messages() {
        let mut c = json!({});
        publish(&mut c, &["one".into()], "connected", 0);
        publish(&mut c, &["two".into()], "connected", 0);
        assert_eq!(c["state"], json!("connected"));
        assert_eq!(c["inbox"], json!(["one", "two"]), "appends, so a slow reader misses nothing");
    }

    #[test]
    fn publish_reports_drops() {
        let mut c = json!({});
        publish(&mut c, &[], "connected", 7);
        assert_eq!(c["dropped"], json!(7));
    }

    #[test]
    fn publish_repairs_a_component_of_the_wrong_shape() {
        let mut c = json!("garbage");
        publish(&mut c, &["x".into()], "connected", 0);
        assert_eq!(c["inbox"], json!(["x"]));
    }

    #[test]
    fn the_script_inbox_is_bounded_per_frame() {
        let mut c = json!({});
        let flood: Vec<String> = (0..MAX_INBOX_PER_FRAME + 50)
            .map(|i| i.to_string())
            .collect();
        publish(&mut c, &flood, "connected", 0);
        assert_eq!(c["inbox"].as_array().unwrap().len(), MAX_INBOX_PER_FRAME);
    }

    #[test]
    fn publishing_nothing_still_updates_state() {
        // A script polling only `state` must see a disconnect even on a frame
        // with no traffic.
        let mut c = json!({ "inbox": [] });
        publish(&mut c, &[], "failed", 0);
        assert_eq!(c["state"], json!("failed"));
    }
}
