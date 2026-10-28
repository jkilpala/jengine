//! **Dialogue** (Oct 2026): conversations as data, played into the scene's
//! own text.
//!
//! A conversation is a **data table** (`dialogue/<name>.data`, as a string
//! table is one under `locale/`), so it reaches every runtime wherever data
//! tables do and no host loads anything new. A row is a node:
//!
//! - `speaker`, `portrait` (a texture), `text` (rich text; `{var}` is
//!   replaced by a variable's value) - a line;
//! - `next` - the node after it (none: the conversation ends);
//! - `choices` - `[{ "text", "next", "if", "set" }]`, the player picks one;
//! - `if` / `else` - a node whose condition fails is passed over to `else`;
//! - `set` - `{ "var": value }` on entering it (`"+1"` / `"-2"` add);
//! - `event` - `on_<event>(entity)` on the runner's own script.
//!
//! A row with no `text` is a step of logic: its `set` and `event` happen and
//! the conversation goes on to `next` at once.
//!
//! A [`Dialogue`] on an entity plays one: it writes the line into the
//! entity named by `text` (its `text` or `ui_text`, with `reveal_speed` - so
//! it types itself out), the speaker's name into `speaker`, the portrait into
//! `portrait` (a `sprite` or `ui_image`) and each choice into one of the
//! `choices` entities (blank when there are fewer). Variables live in the
//! `DialogueVars` custom component of the entity named by `vars` (this one
//! when empty), so villagers sharing one remember together and a script
//! reads and writes them with `Entity.get_custom` / `set_custom`.
//!
//! Scripts drive it with `Dialogue.start(e, node)`, `Dialogue.advance(e)`
//! (finishes a line still typing, else goes on), `Dialogue.choose(e, n)` and
//! `Dialogue.stop(e)`, and read its custom `Dialogue` fields: `playing`,
//! `node`, `speaker`, `line`, `choices` (how many), `typing`. It calls
//! `on_dialogue_line` and `on_dialogue_end` on the runner's script.

use hecs::{Entity, World};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::ecs::components::{CustomComponents, Name, SpriteComponent, TextComponent};

/// The custom component a conversation's variables are kept in.
pub const VARS: &str = "DialogueVars";

/// Plays a conversation from a data table.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Dialogue {
    /// The data table, by its id (`"dialogue/baker"`).
    pub table: String,
    /// The node it starts at.
    pub start: String,
    /// Start on its own on the first frame.
    pub autostart: bool,
    /// The entity whose `DialogueVars` hold the variables (empty: this one).
    pub vars: String,
    /// The entity whose `text` / `ui_text` shows the line.
    pub text: String,
    /// The entity whose text shows the speaker's name.
    pub speaker: String,
    /// The entity whose `sprite` / `ui_image` shows the portrait.
    pub portrait: String,
    /// One entity per choice slot, its text showing that choice.
    pub choices: Vec<String>,
    /// Number the choices ("1. ...").
    pub number_choices: bool,
    /// Characters a second the line types at (0 leaves the text's own).
    pub reveal_speed: f32,
    #[serde(skip)]
    pub state: DialogueState,
}

impl Default for Dialogue {
    fn default() -> Self {
        Self {
            table: String::new(),
            start: "start".into(),
            autostart: false,
            vars: String::new(),
            text: String::new(),
            speaker: String::new(),
            portrait: String::new(),
            choices: Vec::new(),
            number_choices: true,
            reveal_speed: 40.0,
            state: DialogueState::default(),
        }
    }
}

/// Where a conversation is.
#[derive(Debug, Clone, Default)]
pub struct DialogueState {
    pub started: bool,
    pub playing: bool,
    pub node: String,
    pub speaker: String,
    pub line: String,
    /// The choices on offer: `(text, next, set)`.
    pub choices: Vec<(String, String, Option<Value>)>,
    /// Whether they are written into the choice slots yet: not while the
    /// line is still typing.
    pub choices_shown: bool,
}

/// The longest run of logic nodes followed before a line, so a loop of
/// them cannot hang a frame.
const MAX_HOPS: usize = 64;

// ── variables and conditions ────────────────────────────────────────────

fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

fn literal(s: &str) -> Value {
    let s = s.trim();
    if let Ok(n) = s.parse::<f64>() {
        return serde_json::json!(n);
    }
    match s {
        "true" => Value::Bool(true),
        "false" => Value::Bool(false),
        _ => Value::String(s.trim_matches(|c| c == '"' || c == '\'').to_string()),
    }
}

fn compare(a: Option<&Value>, op: &str, b: &Value) -> bool {
    let num = |v: Option<&Value>| match v {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::Bool(b)) => Some(if *b { 1.0 } else { 0.0 }),
        None | Some(Value::Null) => Some(0.0),
        _ => None,
    };
    if let (Some(x), Some(y)) = (num(a), num(Some(b))) {
        return match op {
            ">=" => x >= y,
            "<=" => x <= y,
            ">" => x > y,
            "<" => x < y,
            "==" => x == y,
            "!=" => x != y,
            _ => false,
        };
    }
    let text = |v: Option<&Value>| match v {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
        None => String::new(),
    };
    match op {
        "==" => text(a) == text(Some(b)),
        "!=" => text(a) != text(Some(b)),
        _ => false,
    }
}

/// Whether `cond` holds over `vars`: terms joined by `and` / `or` (`and`
/// binding tighter), each `var`, `!var` or `var <op> value` with `>=`,
/// `<=`, `>`, `<`, `==` or `!=`. An empty condition holds.
pub fn holds(cond: &str, vars: &Map<String, Value>) -> bool {
    let cond = cond.trim();
    if cond.is_empty() {
        return true;
    }
    cond.split(" or ").any(|any| {
        any.split(" and ").all(|term| {
            let term = term.trim();
            for op in [">=", "<=", "==", "!=", ">", "<"] {
                if let Some((l, r)) = term.split_once(op) {
                    return compare(vars.get(l.trim()), op, &literal(r));
                }
            }
            match term.strip_prefix('!') {
                Some(v) => !truthy(vars.get(v.trim())),
                None => truthy(vars.get(term)),
            }
        })
    })
}

/// Apply a node's or choice's `set` to `vars`: `"+n"` / `"-n"` add to a
/// number, anything else is the value.
pub fn apply_set(set: &Value, vars: &mut Map<String, Value>) {
    let Some(set) = set.as_object() else { return };
    for (k, v) in set {
        let add = v.as_str().and_then(|s| {
            let s = s.trim();
            (s.starts_with('+') || s.starts_with('-')).then(|| s.parse::<f64>().ok()).flatten()
        });
        let next = match add {
            Some(d) => {
                let have = vars.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                serde_json::json!(have + d)
            }
            None => v.clone(),
        };
        vars.insert(k.clone(), next);
    }
}

fn shown(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => match n.as_f64() {
            Some(f) if f.fract() == 0.0 && f.abs() < 1e15 => format!("{}", f as i64),
            _ => n.to_string(),
        },
        Value::Null => String::new(),
        v => v.to_string(),
    }
}

/// `text` with every `{var}` replaced by its value (a name it does not
/// know is left as written).
pub fn fill(text: &str, vars: &Map<String, Value>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) if vars.contains_key(after[..close].trim()) => {
                out.push_str(&shown(&vars[after[..close].trim()]));
                rest = &after[close + 1..];
            }
            _ => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

// ── the world ───────────────────────────────────────────────────────────

fn named(world: &World, name: &str) -> Option<Entity> {
    if name.is_empty() {
        return None;
    }
    world.query::<&Name>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e)
}

fn vars_entity(world: &World, e: Entity, d: &Dialogue) -> Entity {
    named(world, &d.vars).unwrap_or(e)
}

fn read_vars(world: &World, holder: Entity) -> Map<String, Value> {
    world
        .get::<&CustomComponents>(holder)
        .ok()
        .and_then(|c| c.0.get(VARS).and_then(Value::as_object).cloned())
        .unwrap_or_default()
}

fn write_custom(world: &mut World, e: Entity, key: &str, v: Value) {
    let missing = match world.get::<&mut CustomComponents>(e) {
        Ok(mut c) => {
            c.0.insert(key.into(), v);
            None
        }
        Err(_) => Some(v),
    };
    if let Some(v) = missing {
        let mut map = std::collections::HashMap::new();
        map.insert(key.into(), v);
        let _ = world.insert_one(e, CustomComponents(map));
    }
}

/// Write `s` into an entity's text (world or UI), restarting its typewriter
/// at `speed` when that is set.
fn set_text(world: &mut World, name: &str, s: &str, speed: f32) {
    let Some(e) = named(world, name) else { return };
    // The typewriter starts over even for a line the same as the last.
    if let Ok(mut t) = world.get::<&mut TextComponent>(e) {
        t.text = s.to_string();
        t.reveal = None;
        if speed > 0.0 {
            t.reveal_speed = speed;
        }
    }
    if let Ok(mut t) = world.get::<&mut crate::ui::UIText>(e) {
        t.text = s.to_string();
        t.reveal = None;
        if speed > 0.0 {
            t.reveal_speed = speed;
        }
    }
}

/// Show `texture` in a portrait slot; none fades the slot out and leaves
/// its last picture (an empty texture id is a missing texture to a host).
fn set_portrait(world: &mut World, name: &str, texture: &str) {
    let Some(e) = named(world, name) else { return };
    let alpha = if texture.is_empty() { 0.0 } else { 1.0 };
    if let Ok(mut s) = world.get::<&mut SpriteComponent>(e) {
        if !texture.is_empty() {
            s.texture_id = texture.into();
        }
        s.color[3] = alpha;
    }
    if let Ok(mut i) = world.get::<&mut crate::ui::UIImage>(e) {
        if !texture.is_empty() {
            i.texture_id = texture.to_string();
        }
        i.color[3] = alpha;
    }
}

/// Every portrait a dialogue's table names: nothing shows one until its
/// line comes up, too late to read a file, so the hosts preload these
/// (`scene::format::world_texture_ids`).
pub fn portrait_ids(d: &Dialogue) -> Vec<String> {
    let Some(data) = crate::data::global() else { return Vec::new() };
    let Some(table) = data.table(&d.table) else { return Vec::new() };
    table
        .rows
        .iter()
        .filter_map(|r| r.fields.get("portrait").and_then(Value::as_str))
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether the line's text is still typing.
fn typing(world: &World, name: &str, line: &str) -> bool {
    let Some(e) = named(world, name) else { return false };
    let (speed, reveal, rich, shown_text) = if let Ok(t) = world.get::<&TextComponent>(e) {
        (t.reveal_speed, t.reveal, t.rich, t.text.clone())
    } else if let Ok(t) = world.get::<&crate::ui::UIText>(e) {
        (t.reveal_speed, t.reveal, t.rich, t.text.clone())
    } else {
        return false;
    };
    if !(speed > 0.0) {
        return false;
    }
    // A clock not yet started on this text: it has everything still to type.
    let Some(reveal) = reveal.filter(|r| r.key == crate::renderer::rich_text::text_key(&shown_text)) else {
        return !line.is_empty();
    };
    let parsed = if rich {
        crate::renderer::rich_text::parse(line, 0)
    } else {
        crate::renderer::rich_text::RichText {
            plain: line.to_string(),
            styles: vec![Default::default(); line.chars().count()],
            ..Default::default()
        }
    };
    let (shown, _) = crate::renderer::rich_text::visible_chars(&parsed, speed, Some(reveal.elapsed));
    shown < parsed.styles.len()
}

/// Show the whole of a line still typing.
fn finish_typing(world: &mut World, name: &str) {
    let Some(e) = named(world, name) else { return };
    let done = |text: &str| {
        Some(crate::ecs::components::TextReveal { key: crate::renderer::rich_text::text_key(text), elapsed: 1.0e6 })
    };
    if let Ok(mut t) = world.get::<&mut TextComponent>(e) {
        t.reveal = done(&t.text);
    }
    if let Ok(mut t) = world.get::<&mut crate::ui::UIText>(e) {
        t.reveal = done(&t.text);
    }
}

fn row(table: &str, node: &str) -> Option<Map<String, Value>> {
    let data = crate::data::global()?;
    data.row(table, node).map(|r| r.fields.clone())
}

fn text_of(row: &Map<String, Value>, key: &str) -> String {
    row.get(key).and_then(Value::as_str).unwrap_or_default().to_string()
}

/// Go to `node` (empty: the end), passing over logic nodes and failed
/// conditions, and show the line it lands on.
fn enter(world: &mut World, e: Entity, mut node: String) {
    let Ok(d) = world.get::<&Dialogue>(e).map(|d| (*d).clone()) else { return };
    let holder = vars_entity(world, e, &d);
    let mut vars = read_vars(world, holder);
    let mut landed: Option<Map<String, Value>> = None;
    for _ in 0..MAX_HOPS {
        if node.is_empty() {
            break;
        }
        let Some(r) = row(&d.table, &node) else {
            log::warn!("Dialogue: no node '{node}' in '{}'", d.table);
            node.clear();
            break;
        };
        if !holds(&text_of(&r, "if"), &vars) {
            node = text_of(&r, "else");
            continue;
        }
        if let Some(set) = r.get("set") {
            apply_set(set, &mut vars);
        }
        let event = text_of(&r, "event");
        if !event.is_empty() {
            crate::ecs::callbacks::push(e, crate::ecs::callbacks::handler_for(&event));
        }
        if r.get("text").and_then(Value::as_str).is_some_and(|t| !t.is_empty()) {
            landed = Some(r);
            break;
        }
        node = text_of(&r, "next");
    }
    write_custom(world, holder, VARS, Value::Object(vars.clone()));
    let mut state = DialogueState { started: true, ..Default::default() };
    match landed {
        Some(r) => {
            state.playing = true;
            state.node = node;
            state.speaker = text_of(&r, "speaker");
            state.line = fill(&text_of(&r, "text"), &vars);
            if let Some(list) = r.get("choices").and_then(Value::as_array) {
                for c in list {
                    let cond = c.get("if").and_then(Value::as_str).unwrap_or_default();
                    if holds(cond, &vars) {
                        let text = fill(c.get("text").and_then(Value::as_str).unwrap_or_default(), &vars);
                        let next = c.get("next").and_then(Value::as_str).unwrap_or_default().to_string();
                        state.choices.push((text, next, c.get("set").cloned()));
                    }
                }
            }
            set_text(world, &d.text, &state.line, d.reveal_speed);
            set_text(world, &d.speaker, &state.speaker, 0.0);
            set_portrait(world, &d.portrait, &text_of(&r, "portrait"));
            crate::ecs::callbacks::push(e, "on_dialogue_line");
        }
        None => {
            set_text(world, &d.text, "", 0.0);
            set_text(world, &d.speaker, "", 0.0);
            set_portrait(world, &d.portrait, "");
            crate::ecs::callbacks::push(e, "on_dialogue_end");
        }
    }
    for slot in &d.choices {
        set_text(world, slot, "", 0.0);
    }
    if let Ok(mut d) = world.get::<&mut Dialogue>(e) {
        d.state = state;
    }
    publish(world, e);
}

/// Write the choices into their slots (numbered unless told not to).
fn show_choices(world: &mut World, d: &Dialogue) {
    for (i, slot) in d.choices.iter().enumerate() {
        let s = match d.state.choices.get(i) {
            Some((t, _, _)) if d.number_choices => format!("{}. {t}", i + 1),
            Some((t, _, _)) => t.clone(),
            None => String::new(),
        };
        set_text(world, slot, &s, 0.0);
    }
}

/// The custom `Dialogue` fields a script reads.
fn publish(world: &mut World, e: Entity) {
    let Ok(d) = world.get::<&Dialogue>(e).map(|d| (*d).clone()) else { return };
    let s = &d.state;
    let still_typing = s.playing && typing(world, &d.text, &s.line);
    // The choices wait for the line.
    if s.playing && !still_typing && !s.choices_shown {
        show_choices(world, &d);
        if let Ok(mut d) = world.get::<&mut Dialogue>(e) {
            d.state.choices_shown = true;
        }
    }
    let v = serde_json::json!({
        "playing": s.playing,
        "node": s.node,
        "speaker": s.speaker,
        "line": s.line,
        "choices": s.choices.len(),
        "typing": still_typing,
    });
    write_custom(world, e, "Dialogue", v);
}

/// What the line after this one is, when it has no choices.
fn next_of(d: &Dialogue) -> String {
    row(&d.table, &d.state.node).map(|r| text_of(&r, "next")).unwrap_or_default()
}

/// Play `node` (empty: the dialogue's `start`).
pub fn start(world: &mut World, e: Entity, node: &str) -> Result<(), String> {
    let d = world.get::<&Dialogue>(e).map_err(|_| "no dialogue on that entity".to_string())?;
    let node = if node.is_empty() { d.start.clone() } else { node.to_string() };
    drop(d);
    enter(world, e, node);
    Ok(())
}

/// Finish a line still typing; else go on to the next (a line with choices
/// waits for one).
pub fn advance(world: &mut World, e: Entity) -> Result<(), String> {
    let d = world.get::<&Dialogue>(e).map(|d| (*d).clone()).map_err(|_| "no dialogue on that entity".to_string())?;
    if !d.state.playing {
        return Ok(());
    }
    if typing(world, &d.text, &d.state.line) {
        finish_typing(world, &d.text);
        publish(world, e);
        return Ok(());
    }
    if d.state.choices.is_empty() {
        enter(world, e, next_of(&d));
    }
    Ok(())
}

/// Take choice `n` (from 1).
pub fn choose(world: &mut World, e: Entity, n: usize) -> Result<(), String> {
    let d = world.get::<&Dialogue>(e).map(|d| (*d).clone()).map_err(|_| "no dialogue on that entity".to_string())?;
    let Some((_, next, set)) = n.checked_sub(1).and_then(|i| d.state.choices.get(i)).cloned() else {
        return Err(format!("no choice {n} on show"));
    };
    if let Some(set) = set {
        let holder = vars_entity(world, e, &d);
        let mut vars = read_vars(world, holder);
        apply_set(&set, &mut vars);
        write_custom(world, holder, VARS, Value::Object(vars));
    }
    enter(world, e, next);
    Ok(())
}

/// End the conversation now.
pub fn stop(world: &mut World, e: Entity) -> Result<(), String> {
    world.get::<&Dialogue>(e).map_err(|_| "no dialogue on that entity".to_string())?;
    enter(world, e, String::new());
    Ok(())
}

/// One script request, as the command carries it: `start` (arg a node),
/// `advance`, `choose` (arg the number) or `stop`.
pub fn op(world: &mut World, e: Entity, op: &str, arg: &str) -> Result<(), String> {
    match op {
        "start" => start(world, e, arg),
        "advance" => advance(world, e),
        "choose" => choose(world, e, arg.trim().parse::<f64>().map_err(|_| format!("choose: '{arg}'"))? as usize),
        "stop" => stop(world, e),
        other => Err(format!("unknown dialogue op '{other}'")),
    }
}

/// Start what autostarts, and keep each runner's `typing` field current.
/// Called from `tick_motion`.
pub fn tick_dialogue(world: &mut World) {
    let runners: Vec<(Entity, bool)> =
        world.query::<&Dialogue>().iter().map(|(e, d)| (e, d.autostart && !d.state.started)).collect();
    for (e, autostart) in runners {
        if autostart {
            let _ = start(world, e, "");
        } else {
            publish(world, e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(v: Value) -> Map<String, Value> {
        v.as_object().unwrap().clone()
    }

    #[test]
    fn conditions_read_the_variables() {
        let v = vars(serde_json::json!({ "gold": 7, "met": true, "mood": "glum" }));
        assert!(holds("", &v));
        assert!(holds("met", &v) && !holds("!met", &v) && holds("!stranger", &v));
        assert!(holds("gold >= 5", &v) && !holds("gold > 7", &v) && holds("gold == 7", &v));
        assert!(holds("mood == glum", &v) && holds("mood != 'happy'", &v));
        assert!(holds("gold < 3 or met", &v) && !holds("gold < 3 and met", &v));
        assert!(holds("met and gold != 0 or stranger", &v));
    }

    #[test]
    fn sets_add_and_replace() {
        let mut v = vars(serde_json::json!({ "gold": 7 }));
        apply_set(&serde_json::json!({ "gold": "-2", "visits": "+1", "met": true, "mood": "glad" }), &mut v);
        assert_eq!(v["gold"].as_f64(), Some(5.0));
        assert_eq!(v["visits"].as_f64(), Some(1.0));
        assert_eq!(v["met"], Value::Bool(true));
        assert_eq!(fill("{gold} coins, {visits} visit, {nobody} {mood}", &v), "5 coins, 1 visit, {nobody} glad");
    }
}
