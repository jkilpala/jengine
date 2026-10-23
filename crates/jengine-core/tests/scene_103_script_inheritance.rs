//! Test-lab scene 103, natively: its own scene file and its own scripts.
//!
//! Three boxes whose scripts carry no spin code of their own, inheriting
//! `init`, `update` and an `on_ping` handler through `script = { extends =
//! "..." }` from a module chain under `scripts/lua/inherit/`. The editor and
//! web half - the same rule in `lua-runtime.ts` - is tested in
//! `editor/src/lib/luaRuntime.test.ts`; this is what the scene promises on
//! native, checked from the files a player would run rather than from
//! snippets.

#![cfg(feature = "native")]

use std::collections::HashMap;
use std::path::PathBuf;

use jengine_core::scripting::bridge::{
    fs_resolver, BridgeCommand, BridgeReadSnapshot, Capabilities, LuauAdapter, ScriptBridgeRunner,
};

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../projects/engine-test-lab/assets")
}

/// `(id, name, script path, source, position)` for each scripted box.
fn boxes() -> Vec<(u64, String, String, String, (f32, f32))> {
    let text = std::fs::read_to_string(assets().join("scenes/103-script-inheritance.scene")).unwrap();
    let scene: serde_json::Value = serde_json::from_str(&text).unwrap();
    let mut out = Vec::new();
    for (i, name) in ["Plain", "Override", "TwoLevels"].iter().enumerate() {
        let e = scene["entities"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["name"] == *name)
            .unwrap_or_else(|| panic!("scene 103 has no {name}"));
        let c = &e["components"];
        let path = c["script"]["source_path"].as_str().unwrap().to_string();
        let source = std::fs::read_to_string(assets().join(&path)).unwrap();
        let p = &c["transform"]["position"];
        let pos = (p[0].as_f64().unwrap() as f32, p[1].as_f64().unwrap() as f32);
        out.push((i as u64 + 1, name.to_string(), path, source, pos));
    }
    out
}

/// Fold a frame's transform and colour writes back into the snapshot the
/// next frame reads, the way a runtime's apply step would.
fn apply(snap: &mut BridgeReadSnapshot, colours: &mut HashMap<u64, [f32; 4]>, cmds: &[BridgeCommand]) {
    for c in cmds {
        match c {
            BridgeCommand::SetRotation { entity_id, radians } => {
                snap.rotations.insert(*entity_id, *radians);
            }
            BridgeCommand::SetScale { entity_id, x, y } => {
                snap.scales.insert(*entity_id, (*x, *y));
            }
            BridgeCommand::SetPosition { entity_id, x, y } => {
                snap.positions.insert(*entity_id, (*x, *y));
            }
            BridgeCommand::SetSpriteColor { entity_id, r, g, b, a } => {
                colours.insert(*entity_id, [*r, *g, *b, *a]);
            }
            _ => {}
        }
    }
}

#[test]
fn every_box_inherits_what_the_scene_says_it_does() {
    let boxes = boxes();
    let mut runner = ScriptBridgeRunner::new(
        Box::new(LuauAdapter::new(Capabilities::empty()).unwrap()),
        Capabilities::empty(),
    );
    runner.set_module_resolver(fs_resolver(assets().join("scripts")));

    let mut snap = BridgeReadSnapshot::default();
    for (id, _, _, _, pos) in &boxes {
        snap.positions.insert(*id, *pos);
        snap.rotations.insert(*id, 0.0);
        snap.scales.insert(*id, (1.0, 1.0));
    }
    let loads: Vec<(u64, String, String)> =
        boxes.iter().map(|(id, _, path, src, _)| (*id, path.clone(), src.clone())).collect();

    let mut colours = HashMap::new();
    let mut listeners: Vec<u64> = Vec::new();
    let dt = 1.0 / 60.0;
    for _ in 0..60 {
        let (cmds, errors) = runner.run_frame(&loads, &snap, dt, &[]);
        assert!(errors.is_empty(), "{errors:?}");
        for c in &cmds {
            if let BridgeCommand::RegisterEventListener { event_name, entity_id, .. } = c
                && event_name == "ping"
            {
                listeners.push(*entity_id);
            }
        }
        apply(&mut snap, &mut colours, &cmds);
    }

    for (id, name, ..) in &boxes {
        // `update` is the spinner's for all three: the middle one's own
        // update reaches it through `super`.
        let turned = snap.rotations[id];
        assert!((turned - 1.5).abs() < 0.05, "{name} should have turned 1.5 rad in a second, turned {turned}");
        // `init` is the spinner's (through `tinted` for the right one), and
        // it is what registers for the ping.
        assert!(listeners.contains(id), "{name} never registered for `ping`: {listeners:?}");
        // The inherited handler.
        let (cmds, err) = runner.dispatch_event(*id, "ping", 0, &snap, dt);
        assert!(err.is_none(), "{name}: {err:?}");
        assert!(
            cmds.iter().any(|c| matches!(c, BridgeCommand::SetScale { x, .. } if (*x - 1.5).abs() < 1e-6)),
            "{name}'s inherited on_ping should pulse it: {cmds:?}"
        );
    }

    // The middle one's own update bobs it; the others never move.
    let (_, _, _, _, start) = &boxes[1];
    assert!((snap.positions[&2].1 - start.1).abs() > 1.0, "Override should bob");
    assert_eq!(snap.positions[&1], boxes[0].4, "Plain should stay put");
    // `tinted`'s init, one level up, before handing on to the spinner's.
    assert_eq!(colours.get(&3), Some(&[1.0, 0.6, 0.2, 1.0]), "TwoLevels should be orange");
    assert_eq!(colours.get(&1), None, "Plain keeps its authored colour");
}
