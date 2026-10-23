//! Localization (`locale`): string tables are data tables under `locale/`,
//! a row per key and a column per language. Lookups fall back, pick plural
//! forms by each language's rule and fill placeholders; `text_key` keeps
//! world and UI text in the current language; scripts ask through
//! `Locale.*`.
//!
//! The language and the tables are process-wide, so every test that
//! publishes tables or sets the language holds `GLOBAL`.

use std::sync::{Arc, Mutex};

use hecs::World;
use jengine_core::data::DataRegistry;
use jengine_core::ecs::behaviors::tick_motion;
use jengine_core::ecs::components::{TextComponent, Transform};
use jengine_core::locale;
use jengine_core::ui::UIText;
use serde_json::json;

static GLOBAL: Mutex<()> = Mutex::new(());

fn tables() -> DataRegistry {
    let mut r = DataRegistry::new();
    let fields: Vec<serde_json::Value> = ["en", "fi", "pl", "fr", "pt-BR"]
        .iter()
        .map(|c| json!({ "name": c, "type": { "kind": "string" }, "default": "" }))
        .collect();
    r.load_schema_json(&json!({ "name": "Strings", "version": 1, "fields": fields }).to_string()).unwrap();
    let row = |id: &str, en: &str, fi: &str, pl: &str, fr: &str| json!({ "id": id, "en": en, "fi": fi, "pl": pl, "fr": fr, "pt-BR": "" });
    r.load_table_json(
        "locale/ui",
        &json!({ "schema": "Strings", "version": 1, "rows": [
            row("_name", "English", "Suomi", "Polski", "Français"),
            row("_decimal", ".", ",", ",", ","),
            row("title", "The Polyglot Inn", "Monikielinen majatalo", "Wielojęzyczna gospoda", "L'Auberge polyglotte"),
            row("greeting", "Welcome, {name}!", "Tervetuloa, {name}!", "Witaj, {name}!", "Bienvenue, {name} !"),
            row("price", "{p:2} coins", "{p:2} kolikkoa", "{p:2} monet", "{p:2} pièces"),
            row("rooms#one", "{n} room free", "{n} huone vapaana", "{n} wolny pokój", "{n} chambre libre"),
            row("rooms#few", "", "", "{n} wolne pokoje", ""),
            row("rooms#many", "", "", "{n} wolnych pokoi", ""),
            row("rooms#other", "{n} rooms free", "{n} huonetta vapaana", "{n} wolnego pokoju", "{n} chambres libres"),
            row("only_en", "Only in English", "", "", ""),
        ]})
        .to_string(),
    )
    .unwrap();
    // A second string table: keys are looked up across all of them.
    r.load_table_json(
        "locale/menu",
        &json!({ "schema": "Strings", "version": 1, "rows": [
            row("play", "Play", "Pelaa", "Graj", "Jouer"),
        ]})
        .to_string(),
    )
    .unwrap();
    // Not under `locale/`: not a string table.
    r.load_table_json(
        "data/items",
        &json!({ "schema": "Strings", "version": 1, "rows": [ row("play", "WRONG", "WRONG", "WRONG", "WRONG") ]}).to_string(),
    )
    .unwrap();
    r
}

#[test]
fn lookups_fall_back_pick_plurals_and_fill_placeholders() {
    let r = tables();
    let t = |lang: &str, key: &str, args: serde_json::Value| locale::translate_with(&r, lang, key, Some(&args));
    assert_eq!(t("fi", "title", json!({})).unwrap(), "Monikielinen majatalo");
    assert_eq!(t("fi", "greeting", json!({ "name": "Aino" })).unwrap(), "Tervetuloa, Aino!");
    assert_eq!(t("fi", "play", json!({})).unwrap(), "Pelaa", "found in the second table");
    // Missing in Finnish: English.
    assert_eq!(t("fi", "only_en", json!({})).unwrap(), "Only in English");
    // Missing everywhere: nothing (translate answers the key).
    assert!(t("fi", "nope", json!({})).is_none());
    // Plurals by each language's rule.
    let rooms = |lang: &str, n: u32| t(lang, "rooms", json!({ "n": n })).unwrap();
    assert_eq!(rooms("en", 1), "1 room free");
    assert_eq!(rooms("en", 3), "3 rooms free");
    assert_eq!(rooms("pl", 1), "1 wolny pokój");
    assert_eq!(rooms("pl", 2), "2 wolne pokoje");
    assert_eq!(rooms("pl", 5), "5 wolnych pokoi");
    assert_eq!(rooms("pl", 22), "22 wolne pokoje");
    assert_eq!(rooms("fr", 0), "0 chambre libre", "French says one for zero");
    assert_eq!(rooms("fi", 2), "2 huonetta vapaana");
    // Each language's decimal separator.
    assert_eq!(t("fi", "price", json!({ "p": 3.5 })).unwrap(), "3,50 kolikkoa");
    assert_eq!(t("en", "price", json!({ "p": 3.5 })).unwrap(), "3.50 coins");
    // A regional language with an empty column falls back through English.
    assert_eq!(t("pt-BR", "play", json!({})).unwrap(), "Play");
    // The languages, alphabetically - from the rows, as the browsers (which
    // load no schemas) see them; data/items is not a string table.
    assert_eq!(locale::available_in(&r), vec!["en", "fi", "fr", "pl", "pt-BR"]);
    let mut schemaless = DataRegistry::new();
    for id in ["locale/ui", "locale/menu"] {
        schemaless.insert_table(id, r.table(id).unwrap().clone());
    }
    assert_eq!(locale::available_in(&schemaless), locale::available_in(&r), "the schema changes nothing");
    assert!(!locale::keys_in(&r).contains("WRONG"));
}

#[test]
fn text_keys_follow_the_language() {
    let _g = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
    jengine_core::data::install_global(Arc::new(tables()));
    locale::set_locale("fi");
    assert_eq!(locale::current(), "fi");
    assert_eq!(locale::language_name("pl"), "Polski");
    assert_eq!(locale::translate("nope", None), "nope", "a missing key shows itself");

    let mut world = World::new();
    let sign = world.spawn((Transform::default(), TextComponent { text: "placeholder".into(), text_key: "title".into(), ..Default::default() }));
    let label = world.spawn((UIText { text_key: "play".into(), ..Default::default() },));
    let plain = world.spawn((Transform::default(), TextComponent { text: "left alone".into(), ..Default::default() }));
    tick_motion(&mut world, 1.0 / 60.0, None, false);
    assert_eq!(world.get::<&TextComponent>(sign).unwrap().text, "Monikielinen majatalo");
    assert_eq!(world.get::<&UIText>(label).unwrap().text, "Pelaa");
    assert_eq!(world.get::<&TextComponent>(plain).unwrap().text, "left alone");

    // Written only when something changed: a script's text stays until then.
    world.get::<&mut TextComponent>(sign).unwrap().text = "set by a script".into();
    tick_motion(&mut world, 1.0 / 60.0, None, false);
    assert_eq!(world.get::<&TextComponent>(sign).unwrap().text, "set by a script");

    locale::set_locale("pl");
    tick_motion(&mut world, 1.0 / 60.0, None, false);
    assert_eq!(world.get::<&TextComponent>(sign).unwrap().text, "Wielojęzyczna gospoda");
    assert_eq!(world.get::<&UIText>(label).unwrap().text, "Graj");

    // New tables are looked up again too.
    let mut more = tables();
    more.load_table_json(
        "locale/menu",
        &json!({ "schema": "Strings", "version": 1, "rows": [
            { "id": "play", "en": "Start", "fi": "Aloita", "pl": "Start", "fr": "Commencer", "pt-BR": "" }
        ]})
        .to_string(),
    )
    .unwrap();
    jengine_core::data::install_global(Arc::new(more));
    tick_motion(&mut world, 1.0 / 60.0, None, false);
    assert_eq!(world.get::<&UIText>(label).unwrap().text, "Start");

    // A language no table has is still the chosen one; lookups fall back.
    locale::set_locale("de");
    assert_eq!(locale::current(), "de");
    assert_eq!(locale::translate("title", None), "The Polyglot Inn");
    // Regional codes find their language.
    locale::set_locale("fi-FI");
    assert_eq!(locale::current(), "fi");
    // Following the system again lands on a language the tables have.
    locale::set_locale("");
    assert!(locale::available().contains(&locale::current()), "{}", locale::current());
}

#[cfg(feature = "native")]
#[test]
fn a_script_asks_in_the_players_language() {
    use jengine_core::ecs::components::{ScriptComponent, ScriptLanguage};
    use jengine_core::ecs::systems::ScriptSystem;
    use jengine_core::scripting::api::ScriptCommand;
    let _g = GLOBAL.lock().unwrap_or_else(|e| e.into_inner());
    jengine_core::data::install_global(Arc::new(tables()));
    locale::set_locale("en");
    let mut world = World::new();
    let sign = world.spawn((Transform::default(), TextComponent::default()));
    let src = format!(
        r#"
function update(entity, dt)
    Locale.set("pl")
    Text.set({id}, Locale.get("rooms", {{ n = 5 }}) .. "|" .. Locale.current() .. "|" .. #Locale.list() .. "|" .. Locale.name("fi"))
end
"#,
        id = sign.id()
    );
    let dir = std::env::temp_dir().join(format!("jengine_locale_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("s.lua");
    std::fs::write(&path, src).unwrap();
    world.spawn((
        Transform::default(),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    system.run(&mut world, 1.0 / 60.0, &jengine_core::input::InputState::new(), None, None);
    let text = system
        .api
        .commands
        .iter()
        .find_map(|c| match c {
            ScriptCommand::SetText { text, .. } => Some(text.clone()),
            _ => None,
        })
        .expect("the script set the text");
    assert_eq!(text, "5 wolnych pokoi|pl|5|Suomi");
    locale::set_locale("");
}

#[test]
fn the_scene_format_keeps_the_key() {
    let mut world = World::new();
    world.spawn((Transform::default(), TextComponent { text: "x".into(), text_key: "title".into(), ..Default::default() }));
    let scene = jengine_core::scene::format::save_scene(&world);
    let json = serde_json::to_string(&scene).unwrap();
    assert!(json.contains("\"text_key\":\"title\""), "{json}");
    assert!(!json.contains("localized"), "the stamp is not saved");
}

#[test]
fn every_runtime_answers_locale() {
    let behaviors = include_str!("../src/ecs/behaviors.rs");
    let body = &behaviors[behaviors.find("pub fn tick_motion(").unwrap()..];
    assert!(body[..body.find("\n}\n").unwrap()].contains("locale::apply_text_keys(world)"));
    let nav = include_str!("../src/scripting/bridge/adapters/js/nav.rs");
    assert!(nav.contains("install_locale_callbacks(&state)"), "the browsers never install Locale.*");
    let hooks = include_str!("../src/scripting/bridge/adapters/js/locale.rs");
    for name in ["locale_get", "locale_set", "locale_current", "locale_list", "locale_name"] {
        assert!(hooks.contains(&format!("\"{name}\"")), "the browsers never answer {name}");
    }
}
