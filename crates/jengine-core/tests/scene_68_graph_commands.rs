//! Test-lab scene 68, end to end: every graph command and trigger it shows.
//!
//! Until Sep 2026 none of this happened in any runtime (BUGS #93):
//! `SpawnEntity` spawned a bare `Name + Transform`; `PlaySound`,
//! `ChangeScene`, `PlayAnimation` and `StopAnimation` were recorded for "the
//! host to forward" and no host did; `OnKey`, `OnMouseClick` and `OnTimer` had
//! no caller; and the browser hosts never pruned instances, so `OnDespawn`
//! never fired there.
//!
//! This drives the frame through the same core calls every runtime now makes
//! - `sync_instances_inline`, `tick_pre_simulation`, `dispatch_input`,
//! `flush_commands_and_route_events` with the project's prefabs - from the
//! project's own files.

use std::collections::HashMap;
use std::path::PathBuf;

use jengine_core::animation::SpriteAnimator;
use jengine_core::animation::asset::{collect_clip_sources, inline_anim_sources, parse_anim_asset};
use jengine_core::behavior::{BehaviorGraphSystem, PrefabSource};
use jengine_core::ecs::components::{BehaviorGraphComponent, Name, SpriteComponent, Transform};
use jengine_core::input::{InputState, KeyCode};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;
use jengine_core::scene::format::EntityData;
use jengine_core::scene::load_scene_auto;

fn assets() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../projects/engine-test-lab/assets")
}

fn find(world: &hecs::World, name: &str) -> Option<hecs::Entity> {
    world.query::<&Name>().iter().find(|(_, n)| n.0 == name).map(|(e, _)| e)
}

fn count(world: &hecs::World, name: &str) -> usize {
    world.query::<&Name>().iter().filter(|(_, n)| n.0 == name).count()
}

struct Lab {
    world: hecs::World,
    registry: ComponentRegistry,
    prefabs: HashMap<String, EntityData>,
    graphs: HashMap<String, String>,
    behaviors: BehaviorGraphSystem,
    input: InputState,
    sounds: Vec<String>,
    scene_changes: Vec<String>,
}

impl Lab {
    fn load() -> Self {
        let mut registry = ComponentRegistry::new();
        register_core_components(&mut registry);

        let text = std::fs::read_to_string(assets().join("scenes/68-graph-commands.scene")).unwrap();
        let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut anims = HashMap::new();
        for src in collect_clip_sources(&json) {
            let bytes = std::fs::read(assets().join(&src)).unwrap();
            anims.insert(src.clone(), parse_anim_asset(&bytes, &src).unwrap());
        }
        assert!(inline_anim_sources(&mut json, &anims).is_empty());
        let mut world = hecs::World::new();
        load_scene_auto(&json.to_string(), &mut world, &registry).expect("load scene 68");

        let mut prefabs = HashMap::new();
        for e in std::fs::read_dir(assets().join("prefabs")).unwrap().flatten() {
            let data: EntityData =
                serde_json::from_str(&std::fs::read_to_string(e.path()).unwrap()).unwrap();
            prefabs.insert(data.name.clone(), data);
        }

        let mut graphs = HashMap::new();
        for (_, c) in world.query::<&BehaviorGraphComponent>().iter() {
            let json = std::fs::read_to_string(assets().join(&c.graph_path)).unwrap();
            graphs.insert(c.graph_path.clone(), json);
        }

        let mut input = InputState::default();
        input.screen_size = (1280.0, 720.0);
        Lab {
            world,
            registry,
            prefabs,
            graphs,
            behaviors: BehaviorGraphSystem::new(),
            input,
            sounds: Vec::new(),
            scene_changes: Vec::new(),
        }
    }

    /// One frame, in the order the runtimes run it.
    fn frame(&mut self, press: Option<KeyCode>, click_at: Option<(f32, f32)>) {
        let dt = 1.0 / 60.0;
        self.input.begin_frame();
        if let Some(k) = press {
            self.input.key_down(k);
        }
        if let Some(p) = click_at {
            self.input.mouse_position = p;
            self.input.set_mouse_button(true, true);
        } else {
            self.input.set_mouse_button(true, false);
        }
        let errors = self.behaviors.sync_instances_inline(&mut self.world, &self.graphs);
        assert!(errors.is_empty(), "graph failed to attach: {errors:?}");
        self.behaviors.tick_pre_simulation(&mut self.world, dt);
        for e in jengine_core::ecs::behaviors::update_lifetimes(&mut self.world, dt) {
            let _ = self.world.despawn(e);
        }
        self.behaviors.dispatch_input(&self.input, &mut self.world);
        let routed = self.behaviors.flush_commands_and_route_events(
            &mut self.world,
            Some(&PrefabSource { registry: &self.registry, prefabs: &self.prefabs }),
        );
        self.sounds.extend(routed.play_sound_requests.into_iter().map(|r| r.sound_id));
        self.scene_changes.extend(routed.change_scene_requests);
        if let Some(k) = press {
            self.input.key_up(k);
        }
    }

    fn run(&mut self, frames: usize) {
        for _ in 0..frames {
            self.frame(None, None);
        }
    }

    fn animator_playing(&self, name: &str) -> bool {
        let e = find(&self.world, name).unwrap();
        self.world.get::<&SpriteAnimator>(e).unwrap().playing
    }
}

#[test]
fn every_graph_in_scene_68_does_what_its_hint_says() {
    let mut lab = Lab::load();
    lab.run(2);
    assert!(lab.behaviors.instances.len() >= 8, "scene 68's graphs did not all attach");

    // S: SpawnEntity builds the real prefab where the graph says, then a blip.
    lab.frame(Some(KeyCode::S), None);
    let coin = find(&lab.world, "Coin").expect("S spawned no Coin");
    assert!(
        lab.world.get::<&SpriteComponent>(coin).is_ok(),
        "the Coin was spawned bare - the prefab's components are missing"
    );
    let at = lab.world.get::<&Transform>(coin).unwrap().position;
    assert!((at.x + 480.0).abs() < 1.0 && (at.y + 20.0).abs() < 1.0, "Coin spawned at {at:?}");
    assert_eq!(lab.sounds, vec!["sounds/blip.wav".to_string()], "S did not play the blip");

    // A / X: the animation ops reach the SpriteAnimator.
    assert!(!lab.animator_playing("SpinA"));
    assert!(lab.animator_playing("SpinB"));
    lab.frame(Some(KeyCode::A), None);
    lab.frame(Some(KeyCode::X), None);
    assert!(lab.animator_playing("SpinA"), "A did not start the orange spin");
    assert!(!lab.animator_playing("SpinB"), "X did not stop the purple spin");

    // Click: a Spark under the pointer, in world space.
    lab.frame(None, Some((640.0 + 100.0, 360.0 - 50.0)));
    let spark = find(&lab.world, "Spark").expect("a click spawned no Spark");
    let at = lab.world.get::<&Transform>(spark).unwrap().position;
    assert!((at.x - 100.0).abs() < 1.0 && (at.y - 50.0).abs() < 1.0, "Spark spawned at {at:?}");

    // OnTimer: after the doomed box's 4 s, the hand has stepped several times.
    lab.run(4 * 60);
    let hand = find(&lab.world, "ClockHand").unwrap();
    let rot = lab.world.get::<&Transform>(hand).unwrap().rotation;
    assert!(rot < -1.5, "the clock hand has not stepped (rotation {rot}) - OnTimer did not fire");

    // OnDespawn: the red box is gone, it played a blip, and its event
    // turned the grey box green.
    assert!(find(&lab.world, "Doomed").is_none(), "the red box outlived its 4 s");
    assert_eq!(lab.sounds.len(), 2, "OnDespawn did not play its blip: {:?}", lab.sounds);
    let witness = find(&lab.world, "Witness").unwrap();
    let c = lab.world.get::<&SpriteComponent>(witness).unwrap().color;
    assert!(c[1] > 0.9 && c[0] < 0.5, "the grey box never heard OnDespawn's event: {c:?}");

    // L: ChangeScene reaches the runtime.
    lab.frame(Some(KeyCode::L), None);
    assert_eq!(lab.scene_changes, vec!["scenes/68-graph-commands.scene".to_string()]);
    assert_eq!(count(&lab.world, "Coin"), 1);
}
