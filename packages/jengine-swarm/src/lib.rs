//! Swarm AI for jEngine — flocking that scales to thousands of entities.
//!
//! A jEngine package: it registers its own components and a system, and the
//! runtime's `PackageHost` runs that system each frame. See
//! `docs/writing-packages.md`.
//!
//! # What it does
//!
//! One entity carries a [`SwarmSpawner`]. The package's system keeps the boid
//! population matched to the spawner's count — spawning and despawning as
//! needed — then flocks them with a spatial grid (see [`flock`]).
//!
//! Spawning from inside the package rather than from a script is deliberate.
//! Creating thousands of entities through the script bridge would be thousands
//! of deferred commands per frame, and the whole point here is to measure the
//! *simulation*, not the bridge.
//!
//! # Changing the count at runtime
//!
//! A script sets `Entity.set_custom(spawner, "Swarm", "count", n)`. The system reads
//! that override in preference to the authored value.
//!
//! # Steering it
//!
//! The same way, and in every runtime: `Swarm`/`target_x`, `target_y` and
//! `target_pull` draw the flock toward a point (a murmuration wheeling over
//! its roost), and `threat_x`, `threat_y`, `threat_radius` make every boid
//! inside the radius flee it (a falcon through a murmuration). The authored
//! `target_pull` and `threat_strength` are the defaults a script's values
//! override; with neither set the flock only keeps to its area, as it
//! always did.
//!
//! Steering goes through `CustomComponents` rather than script commands of
//! its own. That choice predates `register_script_commands` - when it was
//! made, a package's script functions were installed by the native Lua bridge
//! only - and it still has one advantage: a script can read the values back
//! with `Entity.get_custom`, which a command (returning nothing) cannot offer.
//!
//! # Why it owns its integration
//!
//! It writes `Transform` directly instead of steering a `Velocity2D` and
//! letting the engine integrate. The runtimes order their velocity pass
//! differently relative to the package phases — native integrates after
//! `PreUpdate`, the editor viewport before it — so relying on that would make
//! the same scene behave differently in each, which is useless for a scene
//! whose job is comparing their frame rates.

pub mod flock;

use glam::Vec2;
use jengine_core::ecs::components::{CustomComponents, BulkEntity, SpriteComponent, Transform};
use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::script_api::{ScriptApiRegistry, ScriptArg, ScriptValueType};
use jengine_core::package::system_schedule::{SystemPhase, SystemSchedule};
use jengine_core::package::{JEnginePackage, PackageMetadata};
use serde::{Deserialize, Serialize};

pub use flock::FlockParams;

/// Per-boid state. Velocity lives here rather than in `Velocity2D` so the
/// package owns the whole update — see the module docs.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Boid {
    pub vx: f32,
    pub vy: f32,
    /// Where in its wingbeat this boid is, 0..1 - so a flock does not flap
    /// in step. Set at spawn.
    #[serde(default)]
    pub phase: f32,
}

/// Authored once per scene: describes a flock and how big it should be.
///
/// The system reconciles the live boid population to `count` every frame, so
/// changing it (in the inspector, or from a script via the `Swarm`/`count`
/// custom field) resizes the flock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmSpawner {
    /// How many boids there should be.
    pub count: usize,
    /// Half-extents of the area the flock is kept inside.
    pub half_width: f32,
    pub half_height: f32,
    pub max_speed: f32,
    /// How far a boid looks for neighbours. Also the grid's cell size, so
    /// raising it widens the search *and* makes each cell fuller — it is the
    /// single biggest lever on cost.
    pub neighbor_radius: f32,
    /// Below this distance, neighbours push each other apart.
    pub separation_radius: f32,
    pub separation: f32,
    pub alignment: f32,
    pub cohesion: f32,
    /// Most neighbours one boid considers per frame. Bounds the cost in a
    /// dense flock; see `flock::FlockParams::max_neighbors`.
    #[serde(default = "default_max_neighbors")]
    pub max_neighbors: u32,
    /// Sprite size of a spawned boid, in pixels.
    pub size: f32,
    pub color: [f32; 4],
    /// The boid's texture, pointing right (along +x); empty is a plain
    /// rectangle `size` by half of it, as boids always were.
    #[serde(default)]
    pub texture_id: String,
    /// Draw order of the boids.
    #[serde(default = "default_z")]
    pub z_order: i32,
    /// Wingbeats a second: each boid's height swings with its own phase, a
    /// flap that costs a scale write. `0` (the default) is still.
    #[serde(default)]
    pub flap: f32,
    /// How hard the flock is drawn to its target, when a script gives one
    /// (`Swarm`/`target_x`, `target_y`); `Swarm`/`target_pull` overrides it.
    #[serde(default)]
    pub target_pull: f32,
    /// How hard a boid inside the threat's radius flees it.
    #[serde(default = "default_threat_strength")]
    pub threat_strength: f32,
}

fn default_z() -> i32 {
    5
}

fn default_threat_strength() -> f32 {
    900.0
}

impl Default for SwarmSpawner {
    fn default() -> Self {
        let p = FlockParams::default();
        Self {
            count: 500,
            half_width: p.half_width,
            half_height: p.half_height,
            max_speed: p.max_speed,
            neighbor_radius: p.neighbor_radius,
            separation_radius: p.separation_radius,
            separation: p.separation,
            alignment: p.alignment,
            cohesion: p.cohesion,
            max_neighbors: p.max_neighbors,
            size: 4.0,
            color: [0.55, 0.8, 1.0, 1.0],
            texture_id: Default::default(),
            z_order: default_z(),
            flap: 0.0,
            target_pull: 0.0,
            threat_strength: default_threat_strength(),
        }
    }
}

impl SwarmSpawner {
    fn params(&self) -> FlockParams {
        FlockParams {
            half_width: self.half_width,
            half_height: self.half_height,
            max_speed: self.max_speed,
            neighbor_radius: self.neighbor_radius,
            separation_radius: self.separation_radius,
            separation: self.separation,
            alignment: self.alignment,
            cohesion: self.cohesion,
            max_neighbors: self.max_neighbors,
            target: None,
            threat: None,
        }
    }
}

fn default_max_neighbors() -> u32 {
    FlockParams::default().max_neighbors
}

/// The package.
pub struct SwarmPackage;

impl JEnginePackage for SwarmPackage {
    fn metadata(&self) -> PackageMetadata {
        PackageMetadata {
            name: "jengine-swarm".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "Spatial-grid flocking that scales to thousands of entities".to_string(),
            author: String::new(),
            engine_version: Some("0.1.0".to_string()),
            dependencies: vec![],
        }
    }

    fn register_components(&self, registry: &mut ComponentRegistry) {
        registry.register_defaulted::<SwarmSpawner>("swarm_spawner", "Swarm Spawner", "AI");
        registry.register_defaulted::<Boid>("boid", "Boid", "AI");
    }

    fn register_systems(&self, schedule: &mut SystemSchedule) {
        // `PreUpdate`: the flock should steer before anything else reads
        // transforms this frame.
        schedule.add_system("swarm_flock", SystemPhase::PreUpdate, 0, |ctx| {
            let (world, dt, _resources) = ctx.split();
            update_swarm(world, dt);
        });
    }

    fn register_script_api(&self, api: &mut ScriptApiRegistry) {
        // Metadata only. A handler here would be installed by the native Lua
        // bridge and by nothing else, so the portable way to drive the swarm
        // is the `Swarm`/`count` custom field this documents.
        api.add_function(
            "swarm_set_count",
            "jengine-swarm",
            "Resize the flock: set_custom(spawner, \"Swarm\", \"count\", n)",
            vec![ScriptArg { name: "count".into(), arg_type: ScriptValueType::Int }],
            ScriptValueType::Nil,
        );
    }
}

/// Read the spawner, reconcile the population, then flock.
///
/// Public so a host can call it directly — the test lab's native perf harness
/// does, to time the simulation without a window in the way.
pub fn update_swarm(world: &mut hecs::World, dt: f32) {
    // One spawner per world. More than one is a scene mistake rather than a
    // supported setup, so the first wins and the rest are ignored quietly —
    // logging per frame would flood the console.
    let Some((spawner_entity, spawner)) = world
        .query::<&SwarmSpawner>()
        .iter()
        .next()
        .map(|(e, s)| (e, s.clone()))
    else {
        return;
    };

    let custom = |field: &str| {
        world
            .get::<&CustomComponents>(spawner_entity)
            .ok()
            .and_then(|c| c.0.get("Swarm").and_then(|v| v.get(field)).and_then(|v| v.as_f64()))
            .map(|v| v as f32)
    };
    let override_count = custom("count").map(f64::from);
    let desired = desired_count(spawner.count, override_count);
    let mut params = spawner.params();
    if let (Some(x), Some(y)) = (custom("target_x"), custom("target_y")) {
        params.target = Some((Vec2::new(x, y), custom("target_pull").unwrap_or(spawner.target_pull)));
    }
    if let (Some(x), Some(y), Some(r)) = (custom("threat_x"), custom("threat_y"), custom("threat_radius"))
        && r > 0.0
    {
        params.threat = Some((Vec2::new(x, y), r, spawner.threat_strength));
    }

    reconcile_population(world, &spawner, desired);

    // Gather → simulate → scatter. One pass each way over contiguous vectors
    // is far cheaper than steering through the ECS, where every neighbour
    // lookup would be a component fetch.
    let mut positions: Vec<Vec2> = Vec::with_capacity(desired);
    let mut velocities: Vec<Vec2> = Vec::with_capacity(desired);
    for (_, (t, b)) in world.query::<(&Transform, &Boid)>().iter() {
        positions.push(Vec2::new(t.position.x, t.position.y));
        velocities.push(Vec2::new(b.vx, b.vy));
    }

    flock::step(&mut positions, &mut velocities, &params, dt);
    // The wingbeat clock: one per world is enough, the phases do the rest.
    let beat = if spawner.flap > 0.0 {
        let t = FLAP_CLOCK.with(|c| {
            let t = c.get() + dt * spawner.flap;
            c.set(t.fract());
            t
        });
        Some(t)
    } else {
        None
    };

    // Scatter through the same query rather than `world.get` per entity.
    //
    // Holding a `Vec<Entity>` from the gather and looking each one up again
    // cost two random component fetches per boid — 32,000 of them per frame at
    // 16k, each an entity-meta lookup followed by an archetype index. This is
    // one sequential pass over the same archetypes instead.
    //
    // The pairing is by position, which is sound because `flock::step` touches
    // only the local vectors: the world is structurally unchanged between the
    // two queries, so hecs walks the same archetypes in the same order and
    // yields the same boids in the same sequence. The debug assertion below
    // states that assumption rather than leaving it implied.
    let mut i = 0usize;
    for (_, (t, b)) in world.query::<(&mut Transform, &mut Boid)>().iter() {
        let (p, v) = (positions[i], velocities[i]);
        t.position.x = p.x;
        t.position.y = p.y;
        // Point along travel, so the flock reads as directional.
        t.rotation = v.y.atan2(v.x);
        if let Some(beat) = beat {
            // Wings down to up: the silhouette's height, from a sliver to full.
            let s = ((beat + b.phase) * std::f32::consts::TAU).sin().abs();
            t.scale.y = 0.35 + 0.65 * s;
        }
        b.vx = v.x;
        b.vy = v.y;
        i += 1;
    }
    debug_assert_eq!(
        i,
        positions.len(),
        "gather and scatter disagreed on the boid count - the world changed          shape mid-tick and the two passes are no longer aligned"
    );
}

thread_local! {
    /// Where the wingbeat is, in beats (0..1). Per thread rather than per
    /// spawner because there is one flock per world.
    static FLAP_CLOCK: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
}

/// Upper bound on the flock, so a bad number cannot hang the frame.
///
/// Well above anything usable — the point of the scene this was built for is
/// finding where the frame rate falls over, and that happens long before here.
pub const MAX_BOIDS: usize = 200_000;

/// How many boids there should be: the script override if there is one, else
/// the authored count, clamped to [`MAX_BOIDS`].
///
/// Split out from `update_swarm` so the clamp can be tested without actually
/// spawning `MAX_BOIDS` entities — that test took most of a minute in a debug
/// build, which is too slow to keep in the suite for a policy this small.
pub fn desired_count(authored: usize, override_count: Option<f64>) -> usize {
    override_count
        .map(|n| if n.is_finite() { n.max(0.0) as usize } else { 0 })
        .unwrap_or(authored)
        .min(MAX_BOIDS)
}

/// Spawn or despawn boids so the population matches `desired`.
fn reconcile_population(world: &mut hecs::World, spawner: &SwarmSpawner, desired: usize) {
    let current = world.query::<&Boid>().iter().count();

    if current != desired {
        // Logged on every resize, not per frame. Without it, "is the flock
        // actually there?" has no answer outside a debugger - and a scene
        // whose spawner never loaded looks identical to one that is working,
        // right up until you notice the screen is empty.
        log::info!("[swarm] population {current} -> {desired}");
    }

    if current < desired {
        let to_spawn = desired - current;
        // Seeded from the running total so a flock grown in steps does not
        // stack every batch on the same spot.
        for i in 0..to_spawn {
            let seed = (current + i) as u32;
            let x = (flock::hash01(seed.wrapping_mul(3)) * 2.0 - 1.0) * spawner.half_width;
            let y = (flock::hash01(seed.wrapping_mul(5) ^ 0x9E37_79B9) * 2.0 - 1.0)
                * spawner.half_height;
            let angle = flock::hash01(seed.wrapping_mul(7) ^ 0x85EB_CA6B)
                * std::f32::consts::TAU;
            let speed = spawner.max_speed * 0.6;

            let mut transform = Transform::default();
            transform.position.x = x;
            transform.position.y = y;

            world.spawn((
                transform,
                Boid {
                    vx: angle.cos() * speed,
                    vy: angle.sin() * speed,
                    phase: flock::hash01(seed.wrapping_mul(11) ^ 0x2545_F491),
                },
                // Boids are anonymous bulk simulation - no script can name one,
                // so none should pay for the per-frame script mirror. Without
                // this, 8,000 boids cost 31 ms a frame copying transforms into
                // JS objects nothing ever read.
                BulkEntity,
                SpriteComponent {
                    // Explicit rather than `String::new()`. The engine now
                    // resolves an empty id to this anyway, but it did not
                    // always, and an untextured sprite drew nothing at all -
                    // which is how 16,000 boids simulated at 5 ms/frame onto
                    // a blank screen.
                    texture_id: if spawner.texture_id.is_empty() {
                        jengine_core::asset::WHITE_TEXTURE_ID.into()
                    } else {
                        spawner.texture_id.as_str().into()
                    },
                    color: spawner.color,
                    z_order: spawner.z_order,
                    size: Some(if spawner.texture_id.is_empty() {
                        [spawner.size, spawner.size * 0.5]
                    } else {
                        [spawner.size, spawner.size]
                    }),
                    ..Default::default()
                },
            ));
        }
    } else if current > desired {
        let victims: Vec<hecs::Entity> = world
            .query::<&Boid>()
            .iter()
            .map(|(e, _)| e)
            .take(current - desired)
            .collect();
        for e in victims {
            let _ = world.despawn(e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jengine_core::package::host::PackageHost;

    fn world_with_spawner(count: usize) -> (hecs::World, hecs::Entity) {
        let mut world = hecs::World::new();
        let e = world.spawn((
            Transform::default(),
            SwarmSpawner { count, ..Default::default() },
        ));
        (world, e)
    }

    #[test]
    fn the_population_matches_the_spawner_count() {
        let (mut world, _) = world_with_spawner(64);
        update_swarm(&mut world, 1.0 / 60.0);
        assert_eq!(world.query::<&Boid>().iter().count(), 64);
    }

    #[test]
    fn the_flock_resizes_both_ways() {
        let (mut world, spawner) = world_with_spawner(100);
        update_swarm(&mut world, 1.0 / 60.0);
        assert_eq!(world.query::<&Boid>().iter().count(), 100);

        // Grow, then shrink, through the script-facing custom field.
        set_count(&mut world, spawner, 250.0);
        update_swarm(&mut world, 1.0 / 60.0);
        assert_eq!(world.query::<&Boid>().iter().count(), 250);

        set_count(&mut world, spawner, 25.0);
        update_swarm(&mut world, 1.0 / 60.0);
        assert_eq!(world.query::<&Boid>().iter().count(), 25);
    }

    fn set_count(world: &mut hecs::World, spawner: hecs::Entity, n: f64) {
        let custom = CustomComponents(std::collections::HashMap::from([(
            "Swarm".to_string(),
            serde_json::json!({ "count": n }),
        )]));
        let _ = world.insert_one(spawner, custom);
    }

    #[test]
    fn an_absurd_count_is_clamped_rather_than_attempted() {
        // A typo in a script must not try to allocate its way to a hang.
        // Tested on the policy function, not by spawning MAX_BOIDS entities.
        assert_eq!(desired_count(10, Some(1e9)), MAX_BOIDS);
        assert_eq!(desired_count(10, Some(-5.0)), 0, "negative should floor at 0");
        // NaN reaches here from a script doing arithmetic on a nil.
        assert_eq!(desired_count(10, Some(f64::NAN)), 0);
        assert_eq!(desired_count(10, Some(f64::INFINITY)), 0);
        assert_eq!(desired_count(42, None), 42, "no override means the authored count");
        assert_eq!(desired_count(1_000_000, None), MAX_BOIDS, "authored is clamped too");
    }

    #[test]
    fn every_boid_opts_out_of_the_per_frame_script_mirror() {
        // The browser-script hosts copy every entity's transform into JS
        // objects each frame so scripts can call `get_position`. That is
        // O(all entities) whether or not anything reads it - measured at
        // 8,000 boids it was 31 ms of a 52 ms frame, while the flock is
        // anonymous and no script can name a single one of them.
        let (mut world, _) = world_with_spawner(50);
        update_swarm(&mut world, 1.0 / 60.0);

        let unmarked = world
            .query::<&Boid>()
            .without::<&BulkEntity>()
            .iter()
            .count();
        assert_eq!(unmarked, 0, "{unmarked} boids would be mirrored into JS every frame");
    }

    #[test]
    fn boids_move_and_stay_inside_the_area() {
        let (mut world, _) = world_with_spawner(200);
        update_swarm(&mut world, 1.0 / 60.0);
        let before: Vec<(f32, f32)> = world
            .query::<(&Transform, &Boid)>()
            .iter()
            .map(|(_, (t, _))| (t.position.x, t.position.y))
            .collect();

        for _ in 0..120 {
            update_swarm(&mut world, 1.0 / 60.0);
        }

        let after: Vec<(f32, f32)> = world
            .query::<(&Transform, &Boid)>()
            .iter()
            .map(|(_, (t, _))| (t.position.x, t.position.y))
            .collect();
        assert_ne!(before, after, "nothing moved");

        let s = SwarmSpawner::default();
        for (x, y) in after {
            assert!(
                x.abs() <= s.half_width + 0.01 && y.abs() <= s.half_height + 0.01,
                "boid escaped the area at ({x}, {y})"
            );
            assert!(x.is_finite() && y.is_finite(), "boid position went non-finite");
        }
    }

    #[test]
    fn no_spawner_means_no_work_and_no_panic() {
        let mut world = hecs::World::new();
        world.spawn((Transform::default(),));
        update_swarm(&mut world, 1.0 / 60.0);
        assert_eq!(world.query::<&Boid>().iter().count(), 0);
    }

    #[test]
    fn the_package_registers_its_components_and_system() {
        let mut registry = ComponentRegistry::new();
        jengine_core::package::core_registration::register_core_components(&mut registry);
        let mut host = PackageHost::new();
        host.add_package(Box::new(SwarmPackage));
        host.initialize(&mut registry);

        assert!(registry.get("swarm_spawner").is_some());
        assert!(registry.get("boid").is_some());

        // And the system actually runs through the host, in PreUpdate only.
        let mut world = hecs::World::new();
        world.spawn((
            Transform::default(),
            SwarmSpawner { count: 32, ..Default::default() },
        ));
        host.run_phase(SystemPhase::Animation, &mut world, 1.0 / 60.0);
        assert_eq!(world.query::<&Boid>().iter().count(), 0, "ran in the wrong phase");
        host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);
        assert_eq!(world.query::<&Boid>().iter().count(), 32);
    }

    #[test]
    fn test_lab_scene_61_loads_and_flocks() {
        // End to end against the real asset. The test-lab's own scene loader
        // test lives in jengine-core, which cannot depend on this package, so
        // it builds a registry without `swarm_spawner` - under which the key
        // is an unknown component that lands in `CustomComponents` and is
        // dropped. That test would pass on a scene whose swarm never loads.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../projects/engine-test-lab/assets/scenes/61-swarm.scene");
        let json = std::fs::read_to_string(&path).expect("scene 61 exists");

        let mut registry = ComponentRegistry::new();
        jengine_core::package::core_registration::register_core_components(&mut registry);
        let mut host = PackageHost::new();
        host.add_package(Box::new(SwarmPackage));
        host.initialize(&mut registry);

        let mut world = hecs::World::new();
        jengine_core::scene::load_scene_auto(&json, &mut world, &registry)
            .expect("scene 61 parses");

        let spawner = world
            .query::<&SwarmSpawner>()
            .iter()
            .next()
            .map(|(_, s)| s.clone())
            .expect("scene 61 authors a swarm_spawner");
        assert!(spawner.count > 0, "the scene asks for no boids");

        host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);
        assert_eq!(
            world.query::<&Boid>().iter().count(),
            spawner.count,
            "the scene's flock did not reach its authored size"
        );

        // And they are inside the area the scene asked for, not the default.
        for _ in 0..30 {
            host.run_phase(SystemPhase::PreUpdate, &mut world, 1.0 / 60.0);
        }
        for (_, (t, _)) in world.query::<(&Transform, &Boid)>().iter() {
            assert!(
                t.position.x.abs() <= spawner.half_width + 0.01
                    && t.position.y.abs() <= spawner.half_height + 0.01,
                "boid left the scene's area at ({}, {})",
                t.position.x,
                t.position.y
            );
        }
    }

    // The grid's own equivalence-with-brute-force test moved to the engine
    // along with the grid itself: `jengine_core::ecs::spatial`'s
    // `finds_the_same_neighbours_as_brute_force`. This package had a private
    // copy only because nothing shared existed.
}
