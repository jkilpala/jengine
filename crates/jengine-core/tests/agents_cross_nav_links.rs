//! **Navigation links** (Oct 2026; `ecs::nav_links`): ways across that are
//! not walkable ground. Two rooftops with a gap between them, as two
//! `nav_region`s, and `nav_link`s across it, walked by a real agent through
//! `tick_navigation`: it plans across the gap, walks to the link, crosses it
//! (never touching the gap otherwise), says so in its `NavCrossing` fields,
//! and walks on to its target on the far roof. A one-way link is refused the
//! other way, a switched-off one takes no part, and of two links the cheaper
//! way wins. Found by the showcase's Rooftop Runners.

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::ecs::components::{CustomComponents, Transform};
use jengine_core::ecs::nav_links::NavLink;
use jengine_core::navigation::{NavAgent, NavRegion, tick_navigation};

/// Two roofs, 250 wide and 200 deep, either side of a 100-wide gap (x -50
/// to 50).
fn roofs() -> World {
    let mut world = World::new();
    let outline = vec![[-125.0, -100.0], [125.0, -100.0], [125.0, 100.0], [-125.0, 100.0]];
    for x in [-175.0, 175.0] {
        world.spawn((
            Transform { position: Vec2::new(x, 0.0), ..Default::default() },
            NavRegion { outline: outline.clone(), ..Default::default() },
        ));
    }
    world
}

fn link(world: &mut World, from: Vec2, to: Vec2, extra: NavLink) -> Entity {
    world.spawn((Transform { position: from, ..Default::default() }, NavLink { to: to.to_array(), ..extra }))
}

fn runner(world: &mut World, at: Vec2, to: Vec2) -> Entity {
    world.spawn((
        Transform { position: at, ..Default::default() },
        NavAgent { speed: 200.0, target: Some(to.to_array()), ..Default::default() },
    ))
}

fn crossing(world: &World, e: Entity) -> Option<(String, f64)> {
    let c = world.get::<&CustomComponents>(e).ok()?;
    let v = c.0.get("NavCrossing")?;
    let kind = v["kind"].as_str()?.to_string();
    (!kind.is_empty()).then(|| (kind, v["t"].as_f64().unwrap_or(0.0)))
}

#[test]
fn a_runner_jumps_the_gap_and_runs_on() {
    let mut world = roofs();
    link(&mut world, Vec2::new(-60.0, 0.0), Vec2::new(60.0, 0.0), NavLink { duration: 0.4, kind: "jump".into(), ..Default::default() });
    let r = runner(&mut world, Vec2::new(-250.0, 20.0), Vec2::new(250.0, -20.0));
    let mut jumped = false;
    for _ in 0..(60 * 6) {
        tick_navigation(&mut world, 1.0 / 60.0);
        let p = world.get::<&Transform>(r).unwrap().position;
        let mid_air = crossing(&world, r);
        if let Some((kind, t)) = &mid_air {
            assert_eq!(kind, "jump");
            assert!((0.0..1.0).contains(t), "{t}");
            jumped = true;
        } else {
            assert!(p.x < -50.0 || p.x > 50.0, "only ever in the gap while jumping: {p}");
        }
    }
    let p = world.get::<&Transform>(r).unwrap().position;
    assert!(jumped, "it crossed by the link");
    assert!(p.distance(Vec2::new(250.0, -20.0)) < 10.0, "and ran on to its target: {p}");
    assert!(world.get::<&NavAgent>(r).unwrap().target.is_none(), "and stopped there");
}

#[test]
fn a_one_way_link_is_refused_backwards_and_an_off_one_not_used() {
    let mut world = roofs();
    let l = link(&mut world, Vec2::new(-60.0, 0.0), Vec2::new(60.0, 0.0), NavLink { one_way: true, ..Default::default() });
    // With no way across, an agent does what one without links always did:
    // walks to the nearest point of its own roof, and never into the gap.
    let back = runner(&mut world, Vec2::new(250.0, 0.0), Vec2::new(-250.0, 0.0));
    for _ in 0..240 {
        tick_navigation(&mut world, 1.0 / 60.0);
        assert!(world.get::<&Transform>(back).unwrap().position.x > 50.0, "no way back across a one-way link");
    }
    let p = world.get::<&Transform>(back).unwrap().position;
    assert!((p.x - 66.0).abs() < 8.0, "it waits at its own roof's edge, within its arrival distance: {p}");
    world.get::<&mut NavLink>(l).unwrap().enabled = false;
    let there = runner(&mut world, Vec2::new(-250.0, 40.0), Vec2::new(250.0, 40.0));
    for _ in 0..240 {
        tick_navigation(&mut world, 1.0 / 60.0);
        assert!(world.get::<&Transform>(there).unwrap().position.x < -50.0, "switched off, the link is no way across");
    }
}

#[test]
fn the_cheaper_way_across_wins() {
    let mut world = roofs();
    // A short hop at the top, dear; a long plank lower down, cheap.
    link(&mut world, Vec2::new(-60.0, 60.0), Vec2::new(60.0, 60.0), NavLink { cost: 20.0, kind: "hop".into(), ..Default::default() });
    link(&mut world, Vec2::new(-60.0, -60.0), Vec2::new(60.0, -60.0), NavLink { kind: "plank".into(), ..Default::default() });
    let r = runner(&mut world, Vec2::new(-250.0, 60.0), Vec2::new(250.0, 60.0));
    let mut used = None;
    for _ in 0..(60 * 6) {
        tick_navigation(&mut world, 1.0 / 60.0);
        if let Some((kind, _)) = crossing(&world, r) {
            used = Some(kind);
        }
    }
    assert_eq!(used.as_deref(), Some("plank"), "round by the plank, not the dear hop");
}

#[test]
fn a_cheap_zipline_is_set_off_from_its_start() {
    // One open yard; a zipline cheaper than walking runs across it. The
    // runner can see the zipline's far end from where it starts, and must
    // still walk to the start and set off from there.
    let mut world = World::new();
    world.spawn((Transform::default(), NavRegion::default()));
    link(&mut world, Vec2::new(-100.0, 0.0), Vec2::new(100.0, 0.0), NavLink { cost: 0.1, kind: "zip".into(), ..Default::default() });
    // From here the last step to the start ends about two units past it,
    // which a runner not stopping at the start would walk on by.
    let r = runner(&mut world, Vec2::new(-210.0, 60.0), Vec2::new(200.0, 60.0));
    let mut set_off_at = None;
    for _ in 0..(60 * 6) {
        tick_navigation(&mut world, 1.0 / 60.0);
        if set_off_at.is_none()
            && let Some(c) = world.get::<&NavAgent>(r).unwrap().crossing.as_ref()
        {
            set_off_at = Some(Vec2::from(c.from));
        }
    }
    let at = set_off_at.expect("it took the zipline");
    assert!(at.distance(Vec2::new(-100.0, 0.0)) < 0.5, "from the zipline's start, not short of it or wherever it stood: {at}");
    let p = world.get::<&Transform>(r).unwrap().position;
    assert!(p.distance(Vec2::new(200.0, 60.0)) < 10.0, "and on to its target: {p}");
}

#[test]
fn nobody_is_shoved_by_a_runner_in_the_air() {
    // One runner jumps the gap; another stands just past where it lands.
    // While the first is in the air over it, the second stays put.
    let mut world = roofs();
    link(&mut world, Vec2::new(-60.0, 0.0), Vec2::new(60.0, 0.0), NavLink { duration: 0.5, kind: "jump".into(), ..Default::default() });
    let jumper = runner(&mut world, Vec2::new(-120.0, 0.0), Vec2::new(200.0, 0.0));
    let stander = world.spawn((Transform { position: Vec2::new(80.0, 0.0), ..Default::default() }, NavAgent::default()));
    let mut in_air_frames = 0;
    for _ in 0..240 {
        tick_navigation(&mut world, 1.0 / 60.0);
        if crossing(&world, jumper).is_some() {
            in_air_frames += 1;
            let p = world.get::<&Transform>(stander).unwrap().position;
            assert_eq!(p, Vec2::new(80.0, 0.0), "not shoved by one still in the air");
        }
    }
    assert!(in_air_frames > 10, "the jumper was in the air a while: {in_air_frames}");
}
