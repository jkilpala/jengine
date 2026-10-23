//! Localization (Oct 2026; Godot's `TranslationServer`, Unity's Localization
//! string tables): text looked up by key in the player's language.
//!
//! # String tables are data tables
//!
//! Every `.data` table whose id starts with `locale/` is a string table: a row
//! per key (the row's `id`), a column per language (the schema's fields,
//! named by language code - `en`, `fi`, `de`, `pt-BR`). That is the layout
//! translators already work in, the editor already has a spreadsheet for it,
//! and every runtime already loads data tables - so there is no second asset
//! to deliver and nothing for a host to wire. Two keys are special:
//! `_name` (the language's own name, for a language menu) and `_decimal`
//! (its decimal separator, `.` when absent).
//!
//! # Lookups
//!
//! [`translate`] tries the current language, then its base (`pt` for
//! `pt-BR`), then English, then the first code alphabetically, and answers the key
//! itself when no table has it - so a missing string shows on screen as what
//! is missing. With an `n` (or `count`) argument it first tries the plural
//! form `key#one` / `key#few` / `key#many` / `key#other` that the
//! language's rule ([`plural_category`]) picks, so Polish can say 2 pokoje
//! and 5 pokoi. `{name}` is replaced by the argument `name`; `{name:2}` with
//! two decimals; a number is written with the language's `_decimal`.
//!
//! # The current language
//!
//! Until [`set_locale`] is called it is the system's (the browser's
//! `navigator.language`, Windows' user locale, else `LANG`), if any table
//! has it, else English, else the first code alphabetically. It is process-wide, like
//! `data::global`, and lives here rather than in the world - a scene swap
//! keeps it.
//!
//! # Text that follows the language
//!
//! A `text` or `ui_text` with a `text_key` has its `text` written from the
//! key whenever the language or the tables change ([`apply_text_keys`],
//! inside `tick_motion`, so every runtime runs it).

use std::collections::BTreeSet;
use std::sync::RwLock;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

/// The prefix that makes a data table a string table.
pub const TABLE_PREFIX: &str = "locale/";

/// The language set by [`set_locale`]; `None` follows the system.
static CHOSEN: RwLock<Option<String>> = RwLock::new(None);
/// Bumped by every [`set_locale`], so text knows to look again.
static GENERATION: AtomicU64 = AtomicU64::new(1);

/// What the language and the tables are as of now: changes whenever either
/// does, so a `text_key` is looked up again only then.
pub fn stamp() -> u64 {
    GENERATION.load(Ordering::Relaxed).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ crate::data::generation()
}

/// Choose the language; an empty code goes back to following the system.
pub fn set_locale(code: &str) {
    let code = code.trim();
    if let Ok(mut slot) = CHOSEN.write() {
        *slot = if code.is_empty() { None } else { Some(code.to_string()) };
    }
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// The system's language, as a code (`fi`, `en-US`); `None` when there is no
/// way to ask.
pub fn system_language() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        #[cfg(feature = "web")]
        if let Some(lang) = web_sys::window().and_then(|w| w.navigator().language()) {
            return Some(lang);
        }
        return None;
    }
    #[cfg(all(windows, not(target_arch = "wasm32")))]
    {
        unsafe extern "system" {
            fn GetUserDefaultLocaleName(name: *mut u16, len: i32) -> i32;
        }
        let mut buf = [0u16; 85];
        // SAFETY: the buffer is LOCALE_NAME_MAX_LENGTH (85) wide, as the call
        // requires, and it writes at most that many units.
        let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
        if n > 1 {
            return Some(String::from_utf16_lossy(&buf[..(n - 1) as usize]));
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(v) = std::env::var(var) {
                // `fi_FI.UTF-8` -> `fi-FI`.
                let code = v.split('.').next().unwrap_or("").replace('_', "-");
                if !code.is_empty() && code != "C" && code != "POSIX" {
                    return Some(code);
                }
            }
        }
    }
    #[allow(unreachable_code)]
    None
}

fn base(code: &str) -> &str {
    code.split(['-', '_']).next().unwrap_or(code)
}

/// Every string table, in id order (the first to have a key wins).
fn tables(registry: &crate::data::DataRegistry) -> Vec<&crate::data::table::DataTable> {
    let mut ids: Vec<&str> = registry.table_ids().filter(|id| id.starts_with(TABLE_PREFIX)).collect();
    ids.sort_unstable();
    ids.into_iter().filter_map(|id| registry.table(id)).collect()
}

/// The language codes the tables carry, in alphabetical order. Read from
/// the rows, never the schema: the browsers load tables without their
/// schemas, and a list in schema order natively and key order there would
/// be two answers to one question.
pub fn available_in(registry: &crate::data::DataRegistry) -> Vec<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for t in tables(registry) {
        for row in &t.rows {
            out.extend(row.fields.keys().cloned());
        }
    }
    out.into_iter().collect()
}

/// The languages the loaded tables carry.
pub fn available() -> Vec<String> {
    crate::data::global().map(|r| available_in(&r)).unwrap_or_default()
}

/// The best of `available` for `wanted`: itself, then its base, then a
/// regional form of that base (`pt` finds `pt-BR`).
fn best_match(available: &[String], wanted: &str) -> Option<String> {
    let w = wanted.to_ascii_lowercase();
    if let Some(c) = available.iter().find(|c| c.to_ascii_lowercase() == w) {
        return Some(c.clone());
    }
    let b = base(&w).to_string();
    if let Some(c) = available.iter().find(|c| c.to_ascii_lowercase() == b) {
        return Some(c.clone());
    }
    available.iter().find(|c| base(&c.to_ascii_lowercase()) == b).cloned()
}

/// The language in use: the one chosen, else the system's, else English,
/// else the first code alphabetically - each only if a table has it. A chosen language
/// no table has is still answered (its lookups fall back), so a game can
/// show "fr" before its French table arrives.
pub fn current_in(registry: &crate::data::DataRegistry) -> String {
    let available = available_in(registry);
    if let Some(chosen) = CHOSEN.read().ok().and_then(|c| c.clone()) {
        return best_match(&available, &chosen).unwrap_or(chosen);
    }
    system_language()
        .and_then(|s| best_match(&available, &s))
        .or_else(|| best_match(&available, "en"))
        .or_else(|| available.first().cloned())
        .unwrap_or_else(|| "en".to_string())
}

/// The language in use (see [`current_in`]).
pub fn current() -> String {
    match crate::data::global() {
        Some(r) => current_in(&r),
        None => CHOSEN.read().ok().and_then(|c| c.clone()).unwrap_or_else(|| "en".to_string()),
    }
}

/// CLDR's plural category of the integer `n` in `lang`: `one`, `few`,
/// `many` or `other`. The integer rules of the common languages; anything
/// unknown is English's.
pub fn plural_category(lang: &str, n: f64) -> &'static str {
    if n.fract() != 0.0 {
        return "other";
    }
    let n = n.abs() as u64;
    let (m10, m100) = (n % 10, n % 100);
    match base(&lang.to_ascii_lowercase()) {
        "ja" | "zh" | "ko" | "th" | "vi" | "id" | "ms" | "tr" => "other",
        "fr" | "pt" if lang.eq_ignore_ascii_case("pt-PT") => {
            if n == 1 { "one" } else { "other" }
        }
        "fr" | "pt" => {
            if n <= 1 { "one" } else { "other" }
        }
        "pl" => {
            if n == 1 {
                "one"
            } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                "few"
            } else {
                "many"
            }
        }
        "ru" | "uk" | "be" | "hr" | "sr" | "bs" => {
            if m10 == 1 && m100 != 11 {
                "one"
            } else if (2..=4).contains(&m10) && !(12..=14).contains(&m100) {
                "few"
            } else {
                "many"
            }
        }
        "cs" | "sk" => match n {
            1 => "one",
            2..=4 => "few",
            _ => "other",
        },
        "lt" => {
            if m10 == 1 && !(11..=19).contains(&m100) {
                "one"
            } else if (2..=9).contains(&m10) && !(11..=19).contains(&m100) {
                "few"
            } else {
                "other"
            }
        }
        "lv" => {
            if m10 == 0 || (11..=19).contains(&m100) {
                "zero"
            } else if m10 == 1 && m100 != 11 {
                "one"
            } else {
                "other"
            }
        }
        _ => {
            if n == 1 { "one" } else { "other" }
        }
    }
}

/// One cell: `key` in `lang`, from the first table that has it.
fn cell<'a>(tables: &[&'a crate::data::table::DataTable], key: &str, lang: &str) -> Option<&'a str> {
    tables.iter().find_map(|t| t.row(key)?.fields.get(lang)?.as_str().filter(|s| !s.is_empty()))
}

/// The languages a lookup tries, best first.
fn chain(registry: &crate::data::DataRegistry, lang: &str) -> Vec<String> {
    let available = available_in(registry);
    let mut out = vec![lang.to_string()];
    let b = base(lang).to_string();
    for c in [Some(b), best_match(&available, "en"), available.first().cloned()].into_iter().flatten() {
        if !out.contains(&c) {
            out.push(c);
        }
    }
    out
}

fn count_arg(args: Option<&Value>) -> Option<f64> {
    let a = args?.as_object()?;
    a.get("n").or_else(|| a.get("count")).and_then(Value::as_f64)
}

/// `key` in `lang` (with its fallbacks), plural form and placeholders
/// applied; `None` when no table has it in any of them.
pub fn translate_with(registry: &crate::data::DataRegistry, lang: &str, key: &str, args: Option<&Value>) -> Option<String> {
    let tables = tables(registry);
    let n = count_arg(args);
    for l in chain(registry, lang) {
        let found = n
            .and_then(|n| {
                let form = plural_category(&l, n);
                cell(&tables, &format!("{key}#{form}"), &l).or_else(|| cell(&tables, &format!("{key}#other"), &l))
            })
            .or_else(|| cell(&tables, key, &l));
        if let Some(text) = found {
            let decimal = cell(&tables, "_decimal", &l).unwrap_or(".");
            return Some(substitute(text, args, decimal));
        }
    }
    None
}

/// `key` in the current language - the key itself when nothing has it.
pub fn translate(key: &str, args: Option<&Value>) -> String {
    let Some(registry) = crate::data::global() else { return key.to_string() };
    let lang = current_in(&registry);
    translate_with(&registry, &lang, key, args).unwrap_or_else(|| key.to_string())
}

/// A language's own name (its `_name`), or its code.
pub fn language_name(code: &str) -> String {
    let Some(registry) = crate::data::global() else { return code.to_string() };
    let tables = tables(&registry);
    cell(&tables, "_name", code).map(str::to_string).unwrap_or_else(|| code.to_string())
}

/// A number as the language writes it: whole numbers plainly, others to
/// `decimals` places (or up to two, trailing zeros dropped), with its
/// decimal separator.
pub fn format_number(v: f64, decimals: Option<usize>, decimal: &str) -> String {
    let s = match decimals {
        Some(d) => format!("{v:.d$}"),
        None if v.fract() == 0.0 && v.abs() < 1e15 => format!("{}", v as i64),
        None => {
            let s = format!("{v:.2}");
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        }
    };
    if decimal == "." { s } else { s.replacen('.', decimal, 1) }
}

/// `{name}` and `{name:2}` replaced from `args`; `{{` and `}}` are braces.
/// A placeholder with no argument is left as written.
pub fn substitute(text: &str, args: Option<&Value>, decimal: &str) -> String {
    if !text.contains(['{', '}']) {
        return text.to_string();
    }
    let args = args.and_then(Value::as_object);
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(['{', '}']) {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        if tail.starts_with("{{") || tail.starts_with("}}") {
            out.push_str(&tail[..1]);
            rest = &tail[2..];
            continue;
        }
        if tail.starts_with('}') {
            out.push('}');
            rest = &tail[1..];
            continue;
        }
        let Some(end) = tail.find('}') else {
            out.push_str(tail);
            rest = "";
            break;
        };
        let inner = &tail[1..end];
        let (name, decimals) = match inner.split_once(':') {
            Some((n, d)) => (n.trim(), d.trim().parse::<usize>().ok()),
            None => (inner.trim(), None),
        };
        match args.and_then(|a| a.get(name)) {
            Some(Value::Number(num)) => out.push_str(&format_number(num.as_f64().unwrap_or(0.0), decimals, decimal)),
            Some(Value::String(s)) => out.push_str(s),
            Some(Value::Bool(b)) => out.push_str(if *b { "true" } else { "false" }),
            _ => out.push_str(&tail[..=end]),
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Write every `text_key`'s text in the current language, where the
/// language or the tables changed since it was last written. Called by
/// `behaviors::tick_motion`.
pub fn apply_text_keys(world: &mut hecs::World) {
    let stamp = stamp();
    let mut pending = false;
    for (_, t) in world.query::<&crate::ecs::components::TextComponent>().iter() {
        if !t.text_key.is_empty() && t.localized != stamp {
            pending = true;
            break;
        }
    }
    if !pending {
        for (_, t) in world.query::<&crate::ui::UIText>().iter() {
            if !t.text_key.is_empty() && t.localized != stamp {
                pending = true;
                break;
            }
        }
    }
    if !pending {
        return;
    }
    let registry = crate::data::global();
    let lang = registry.as_deref().map(current_in).unwrap_or_else(current);
    let look = |key: &str| -> String {
        registry
            .as_deref()
            .and_then(|r| translate_with(r, &lang, key, None))
            .unwrap_or_else(|| key.to_string())
    };
    for (_, t) in world.query_mut::<&mut crate::ecs::components::TextComponent>() {
        if !t.text_key.is_empty() && t.localized != stamp {
            let text = look(&t.text_key);
            if t.text != text {
                t.text = text;
            }
            t.localized = stamp;
        }
    }
    for (_, t) in world.query_mut::<&mut crate::ui::UIText>() {
        if !t.text_key.is_empty() && t.localized != stamp {
            let text = look(&t.text_key);
            if t.text != text {
                t.text = text;
            }
            t.localized = stamp;
        }
    }
}

/// Every key the string tables carry (for tools: what to translate).
pub fn keys_in(registry: &crate::data::DataRegistry) -> BTreeSet<String> {
    tables(registry).iter().flat_map(|t| t.rows.iter().map(|r| r.id.clone())).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plural_rules() {
        assert_eq!(plural_category("en", 1.0), "one");
        assert_eq!(plural_category("en", 0.0), "other");
        assert_eq!(plural_category("fr", 0.0), "one");
        assert_eq!(plural_category("pl", 2.0), "few");
        assert_eq!(plural_category("pl", 12.0), "many");
        assert_eq!(plural_category("pl", 22.0), "few");
        assert_eq!(plural_category("pl", 5.0), "many");
        assert_eq!(plural_category("cs", 3.0), "few");
        assert_eq!(plural_category("ru", 21.0), "one");
        assert_eq!(plural_category("ja", 1.0), "other");
        assert_eq!(plural_category("en", 1.5), "other");
    }

    #[test]
    fn placeholders() {
        let args = serde_json::json!({ "name": "Ada", "price": 3.5, "n": 3 });
        assert_eq!(substitute("Hi {name}, {n} rooms at {price:2} €", Some(&args), ","), "Hi Ada, 3 rooms at 3,50 €");
        assert_eq!(substitute("{{literal}} {missing}", Some(&args), "."), "{literal} {missing}");
        assert_eq!(format_number(2.0, None, ","), "2");
        assert_eq!(format_number(2.25, None, ","), "2,25");
    }
}
