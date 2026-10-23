//! Custom-component fields holding tables survive the native Luau bridge.
//!
//! `get_custom` returned nil for any array or object field on native, and
//! `set_custom` / `set_fields` stored a Lua table as null - while the editor
//! and web runtimes passed both through. Packages that publish a list for
//! scripts (a paddle game's `BallState.events`, jengine-net's `inbox`)
//! therefore worked in the editor and were silently empty in a native build.
//! Found running a paddle game natively: the ball bounced, and the
//! script never heard about it.

#![cfg(feature = "native")]

use hecs::World;
use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
use jengine_core::ecs::systems::ScriptSystem;
use jengine_core::input::InputState;
use serde_json::json;

const SRC: &str = r#"
function update(entity, dt)
    local events = get_custom(entity, "Channel", "events")
    if type(events) == "table" then
        set_custom(entity, "Probe", "count", #events)
        set_custom(entity, "Probe", "second_kind", events[2].kind)
        set_custom(entity, "Probe", "second_x", events[2].x)
    else
        set_custom(entity, "Probe", "count", -1)
    end
    local nested = get_custom(entity, "Channel", "nested")
    if type(nested) == "table" then
        set_custom(entity, "Probe", "nested_b", nested.inner.b)
    end
    set_custom(entity, "Probe", "list", { 3, 1, 2 })
    set_custom(entity, "Probe", "map", { name = "pong", hits = 4 })
    set_fields(entity, "Probe", { batch_list = { "a", "b" } })
    -- An empty table is an empty list in every runtime (it was null here).
    set_custom(entity, "Probe", "empty", {})
    -- Booleans stay booleans (BUGS #108): they came back as 1 / 0, so
    -- `== true` was never true and `false` tested truthy.
    local yes, no = get_custom(entity, "Channel", "yes"), get_custom(entity, "Channel", "no")
    set_custom(entity, "Probe", "yes_is_true", yes == true)
    set_custom(entity, "Probe", "no_is_falsy", not no)
    set_custom(entity, "Probe", "yes_type", type(yes))
end
"#;

#[test]
fn tables_round_trip_through_custom_components() {
    // A directory unique to this run. It used to be a fixed name under the
    // system temp dir, which made the test **flaky** under `cargo test
    // --workspace`: the binaries run in parallel, and on Windows a file that
    // has just been written can still be held briefly by the filesystem - so
    // the write, or a reader racing it, occasionally failed. Seen once in a
    // 2566-test run and not reproducible alone, which is the worst kind.
    let dir = std::env::temp_dir().join(format!(
        "jengine_custom_tables_test_{}_{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tables.lua");
    std::fs::write(&path, SRC).unwrap();
    // Best-effort cleanup: a leaked temp directory per run would accumulate.
    struct Clean(std::path::PathBuf);
    impl Drop for Clean {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _clean = Clean(dir.clone());

    let mut custom = CustomComponents::default();
    custom.0.insert(
        "Channel".into(),
        json!({
            "events": [ { "kind": "wall", "x": 1.0 }, { "kind": "paddle", "x": 7.5 } ],
            "nested": { "inner": { "b": 9.0 } },
            "yes": true,
            "no": false,
        }),
    );
    let mut world = World::new();
    let e = world.spawn((
        Name("Probe".into()),
        Transform::default(),
        custom,
        ScriptComponent {
            language: ScriptLanguage::Lua,
            source_path: path.to_string_lossy().into(),
            cull_distance: None,
        },
    ));

    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..3 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }

    let custom = world.get::<&CustomComponents>(e).unwrap();
    let probe = custom.0.get("Probe").expect("the script wrote its readings");
    assert_eq!(probe["count"], json!(2.0), "get_custom returned no table for an array: {probe}");
    assert_eq!(probe["second_kind"], json!("paddle"));
    assert_eq!(probe["second_x"], json!(7.5));
    assert_eq!(probe["nested_b"], json!(9.0), "nested objects: {probe}");
    // Whole Lua numbers arrive as integers, as they always did in the
    // browsers (`lua_number_json`, Oct 2026).
    assert_eq!(probe["list"], json!([3, 1, 2]), "set_custom stored a table as {}", probe["list"]);
    assert_eq!(probe["map"], json!({ "name": "pong", "hits": 4 }));
    assert_eq!(probe["batch_list"], json!(["a", "b"]), "set_fields stored a table as {}", probe["batch_list"]);
    assert_eq!(probe["empty"], json!([]), "an empty table is an empty list, as in the browsers: {}", probe["empty"]);
    assert_eq!(probe["yes_type"], json!("boolean"), "a boolean field came back as {}", probe["yes_type"]);
    assert_eq!(probe["yes_is_true"], json!(true));
    assert_eq!(probe["no_is_falsy"], json!(true), "a false field tested truthy");
}
