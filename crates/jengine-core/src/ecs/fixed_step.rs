//! Gameplay in fixed steps: the same simulation whatever the display does
//! (docs/determinism-proposal.md, phase 4).
//!
//! A host measures one `dt` per display frame and, by default, runs its
//! whole frame once with it - motion, scripts, timers, animation. That ties
//! the simulation to the display: a 165 Hz screen integrates 165 small steps
//! a second and a 60 Hz one 60 larger ones, so the same inputs land a jump
//! in a different place, and two machines in a match that sends inputs alone
//! drift apart within seconds. (Physics alone has always had its own fixed
//! step; it was the rest of the frame that was not.)
//!
//! A project opts in with `settings.simulation_hz`. Then each host asks
//! [`FixedStep::plan`] how many steps the frame's real time pays for - zero
//! on most frames of a 165 Hz display, two after a slow one - and runs its
//! simulation block (motion through physics, triggers, animation, scripts,
//! timers and tweens) that many times, each with exactly `1 / hz` seconds
//! times the time scale. What only shows things - input polling, the
//! camera, audio, particles, drawing - still runs once a frame. Off, the
//! plan is one step of the frame's own `dt`: the host's frame exactly as it
//! was, through the same code.
//!
//! Two things keep that looking and feeling right:
//!
//! - **Input is latched** (`InputState::hold_edges` / `clear_edges`): a press
//!   is seen by exactly one step - kept over frames that run none, and not
//!   seen again by a frame's second step.
//! - **What is drawn is interpolated** ([`FixedStep::blend_for_draw`]): every
//!   entity a step moved is drawn between where it was before the last step
//!   and where it is now, by how far the accumulator has run into the next
//!   step - as the physics has always blended its bodies - so 60 steps a
//!   second still move smoothly at 165 Hz. The true pose is put back at the
//!   start of the next frame ([`FixedStep::restore`]).

use std::cell::Cell;
use std::collections::HashMap;

use hecs::{Entity, World};

use super::components::Transform;

/// The most steps one frame may run. A frame slower than this many steps
/// drops the rest of its time rather than trying to catch up, which on a
/// slow machine would make every frame slower still.
pub const MAX_STEPS_PER_FRAME: u32 = 8;

thread_local! {
    /// The last step that may run ([`set_gate`]); `None` is no gate.
    static GATE: Cell<Option<u64>> = const { Cell::new(None) };
    /// Steps a frame may run beyond what real time pays for while the gate
    /// is ahead ([`set_catch_up`]); 0 is none.
    static CATCH_UP: Cell<u32> = const { Cell::new(0) };
}

/// Let a frame run up to `steps` steps whatever real time says, while the
/// gate is ahead of the clock: how a machine that joins a lockstep match
/// late - a rejoiner, a spectator - replays the match so far from its
/// freshly loaded scene, fast, instead of at the match's own pace forever
/// behind. 0 turns it off; every scene load does ([`FixedStep::reset`]).
pub fn set_catch_up(steps: u32) {
    CATCH_UP.with(|c| c.set(steps));
}

/// Let the fixed-step clock run steps up to `through` and no further - what
/// a lockstep session (a `lockstep_session` component) sets as every
/// player's input for a step arrives, so no machine steps ahead of what all
/// of them know. Steps are counted from the scene's load (`FixedStep::count`).
/// The time a gated frame could not spend is kept (up to
/// [`MAX_STEPS_PER_FRAME`] steps of it) and run when the gate opens. `None`
/// lifts it; every scene load lifts it too ([`FixedStep::reset`]). Process
/// wide, as the replay and display settings are: one host per thread.
pub fn set_gate(through: Option<u64>) {
    GATE.with(|g| g.set(through));
}

/// The step gate, if one is set.
pub fn gate() -> Option<u64> {
    GATE.with(|g| g.get())
}

/// A fingerprint of the world for two machines running one simulation to
/// compare: every root entity's id and transform, in id order. Roots only:
/// a skeleton's bones are posed once per *frame* for drawing, so they differ
/// between displays while the game does not - and they follow their root.
pub fn world_hash(world: &World) -> u64 {
    let mut rows: Vec<(u32, [u32; 5])> = world
        .query::<(&Transform, Option<&super::components::Parent>)>()
        .iter()
        .filter(|(_, (_, parent))| parent.is_none())
        .map(|(e, (t, _))| {
            (e.id(), [t.position.x.to_bits(), t.position.y.to_bits(), t.rotation.to_bits(), t.scale.x.to_bits(), t.scale.y.to_bits()])
        })
        .collect();
    rows.sort_unstable_by_key(|r| r.0);
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut mix = |v: u32| {
        for b in v.to_le_bytes() {
            h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    for (id, bits) in rows {
        mix(id);
        for b in bits {
            mix(b);
        }
    }
    h
}

/// What one frame runs: `steps` simulation steps of `dt` seconds each.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StepPlan {
    pub steps: u32,
    pub dt: f32,
}

/// A host's fixed-step clock and the interpolation that hides it. One per
/// host, kept across frames; [`FixedStep::reset`] when the world is replaced.
#[derive(Debug, Default)]
pub struct FixedStep {
    /// Steps a second; 0 is off. The scene's, if it chose, else the project's.
    hz: f32,
    /// `settings.simulation_hz`; 0 is off.
    project_hz: f32,
    /// The scene's own `simulation_hz`, which wins over the project's while
    /// it is loaded (0 turns fixed steps off for that scene).
    scene_hz: Option<f32>,
    /// Real time not yet paid for by a step, in seconds.
    acc: f64,
    /// Steps run since the last reset.
    count: u64,
    /// Every transform as it was when the latest step began.
    before: HashMap<Entity, Transform>,
    /// What [`Self::blend_for_draw`] wrote, and the true pose under it.
    drawn: Vec<(Entity, Transform, Transform)>,
}

impl FixedStep {
    /// Off: one step a frame of the frame's own `dt`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Steps a second, from `settings.simulation_hz`; 0 (or anything not a
    /// positive number) turns it off. Clamped to 10..=1000 Hz as the
    /// physics step is.
    pub fn set_hz(&mut self, hz: f32) {
        self.project_hz = sanitize(hz);
        self.apply();
    }

    /// A scene was loaded: its own `simulation_hz` ([`scene_simulation_hz`]),
    /// if it has one, for as long as it is loaded - else the project's - and
    /// a fresh clock ([`Self::reset`]). How a single scene opts in (a test
    /// scene, a mini-game) or out (a menu) without changing the project.
    pub fn enter_scene(&mut self, scene_hz: Option<f32>) {
        self.reset();
        self.scene_hz = scene_hz.map(sanitize);
        self.apply();
    }

    /// The loaded scene's own rate, if it chose one: what a pushed scene's
    /// snapshot keeps, so a pop restores it.
    pub fn scene_hz(&self) -> Option<f32> {
        self.scene_hz
    }

    fn apply(&mut self) {
        self.hz = self.scene_hz.unwrap_or(self.project_hz);
        self.acc = 0.0;
    }

    /// From a project's `settings`: `simulation_hz`, absent or 0 for off.
    pub fn from_settings(settings: Option<&serde_json::Value>) -> Self {
        let mut f = Self::new();
        if let Some(hz) = settings.and_then(|s| s.get("simulation_hz")).and_then(|v| v.as_f64()) {
            f.set_hz(hz as f32);
        }
        f
    }

    pub fn hz(&self) -> f32 {
        self.hz
    }

    pub fn is_on(&self) -> bool {
        self.hz > 0.0
    }

    /// Steps run since the last reset: the simulation's own clock, which two
    /// machines running the same match agree on.
    pub fn count(&self) -> u64 {
        self.count
    }

    /// A new world: no time owed, nothing to interpolate.
    pub fn reset(&mut self) {
        set_gate(None);
        set_catch_up(0);
        self.acc = 0.0;
        self.count = 0;
        self.before.clear();
        self.drawn.clear();
    }

    /// The frame's steps. `real_dt` is the frame's paced, clamped wall time
    /// and `scale` the world's time scale. On, the step count follows real
    /// time and each step advances `1 / hz * scale` - so slow motion and a
    /// hitstop (scale 0) still run steps, with a shorter or no `dt`, and a
    /// script counting real time through them keeps counting.
    pub fn plan(&mut self, real_dt: f32, scale: f32) -> StepPlan {
        if !self.is_on() {
            return StepPlan { steps: 1, dt: real_dt * scale };
        }
        let step = 1.0 / self.hz as f64;
        self.acc += real_dt.max(0.0) as f64;
        // A frame time is an `f32`: 0.04 widens to a hair under 0.04, and two
        // steps' worth would floor to one. A millionth of a step forgives it.
        let mut steps = (self.acc / step + 1e-6).floor() as u32;
        if steps > MAX_STEPS_PER_FRAME {
            steps = MAX_STEPS_PER_FRAME;
            self.acc = step * steps as f64;
        }
        // A lockstep gate: no step past the last one every player's input
        // has reached. The time is kept for when it opens, up to a frame's
        // worth of steps.
        if let Some(through) = gate() {
            let allowed = through.saturating_sub(self.count).min(u32::MAX as u64) as u32;
            if steps > allowed {
                steps = allowed;
            }
            // Catching up: run what the gate allows, up to the allowance,
            // without spending (or owing) real time for the extra steps.
            let catch_up = CATCH_UP.with(|c| c.get()).min(allowed);
            if catch_up > steps {
                let extra = catch_up - steps;
                self.acc += step * extra as f64;
                steps = catch_up;
            }
        }
        self.acc = (self.acc - step * steps as f64).clamp(0.0, step * MAX_STEPS_PER_FRAME as f64);
        self.count += steps as u64;
        StepPlan { steps, dt: (step as f32) * scale }
    }

    /// How far real time has run into the next step, 0..1: where between the
    /// last two steps the world is drawn.
    pub fn alpha(&self) -> f32 {
        if !self.is_on() {
            return 1.0;
        }
        ((self.acc * self.hz as f64) as f32).clamp(0.0, 1.0)
    }

    /// Call as each step begins: remember every transform, so the frame can
    /// be drawn between this step's start and its end. Nothing when off.
    pub fn begin_step(&mut self, world: &World) {
        if !self.is_on() {
            return;
        }
        self.before.clear();
        for (e, t) in world.query::<&Transform>().iter() {
            self.before.insert(e, t.clone());
        }
    }

    /// Before drawing: move every entity the last step moved back toward
    /// where it was, by [`Self::alpha`]. Only what moved is touched, and a
    /// thing that did not exist when the step began is drawn where it is.
    /// The physics' own blend is redundant with this, and a host skips it
    /// while this is on.
    pub fn blend_for_draw(&mut self, world: &mut World) {
        self.drawn.clear();
        if !self.is_on() {
            return;
        }
        let a = self.alpha();
        for (e, t) in world.query_mut::<&mut Transform>() {
            let Some(b) = self.before.get(&e) else { continue };
            if transforms_equal(b, t) {
                continue;
            }
            let truth = t.clone();
            *t = lerp_transform(b, &truth, a);
            self.drawn.push((e, t.clone(), truth));
        }
    }

    /// First thing in a frame: put back the true pose of everything
    /// [`Self::blend_for_draw`] moved - unless something has written the
    /// transform since (a skeleton posing its bones after the blend), in
    /// which case that write stands.
    pub fn restore(&mut self, world: &mut World) {
        for (e, drawn, truth) in self.drawn.drain(..) {
            if let Ok(mut t) = world.get::<&mut Transform>(e) {
                if transforms_equal(&t, &drawn) {
                    *t = truth;
                }
            }
        }
    }
}

/// A scene's own `simulation_hz`, from its header (the scene's top-level
/// fields): `Some(0.0)` turns fixed steps off for that scene.
pub fn scene_simulation_hz(header: &serde_json::Value) -> Option<f32> {
    header.get("simulation_hz").and_then(|v| v.as_f64()).map(|v| v as f32)
}

/// A rate as steps a second: anything not a positive number is off, the
/// rest clamped to 10..=1000 Hz as the physics step is.
fn sanitize(hz: f32) -> f32 {
    if hz.is_finite() && hz > 0.0 { hz.clamp(10.0, 1000.0) } else { 0.0 }
}

fn transforms_equal(a: &Transform, b: &Transform) -> bool {
    a.position == b.position && a.rotation == b.rotation && a.scale == b.scale
}

/// `a` toward `b` by `t`; the rotation the short way round.
fn lerp_transform(a: &Transform, b: &Transform, t: f32) -> Transform {
    let mut turn = (b.rotation - a.rotation) % std::f32::consts::TAU;
    if turn > std::f32::consts::PI {
        turn -= std::f32::consts::TAU;
    } else if turn < -std::f32::consts::PI {
        turn += std::f32::consts::TAU;
    }
    let mut out = b.clone();
    out.position = a.position + (b.position - a.position) * t;
    out.rotation = a.rotation + turn * t;
    out.scale = a.scale + (b.scale - a.scale) * t;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec2;

    fn at(x: f32) -> Transform {
        Transform { position: Vec2::new(x, 0.0), ..Default::default() }
    }

    #[test]
    fn off_is_one_step_of_the_frame() {
        let mut f = FixedStep::new();
        assert_eq!(f.plan(0.007, 1.0), StepPlan { steps: 1, dt: 0.007 });
        assert_eq!(f.plan(0.02, 0.5), StepPlan { steps: 1, dt: 0.01 });
        assert_eq!(f.alpha(), 1.0);
    }

    /// The point of it: the same real time gives the same steps, whatever
    /// the frames it arrived in.
    #[test]
    fn steps_follow_real_time_not_frames() {
        let run = |frame: f32, frames: usize| {
            let mut f = FixedStep::new();
            f.set_hz(60.0);
            let plans: Vec<StepPlan> = (0..frames).map(|_| f.plan(frame, 1.0)).collect();
            (plans.iter().map(|p| p.steps).sum::<u32>(), plans)
        };
        // One second at 165, 144, 60 and 30 frames a second.
        for (frame, frames) in [(1.0 / 165.0, 165), (1.0 / 144.0, 144), (1.0 / 60.0, 60), (1.0 / 30.0, 30)] {
            let (steps, plans) = run(frame, frames);
            assert!((59..=60).contains(&steps), "{frames} fps ran {steps} steps");
            assert!(plans.iter().all(|p| p.dt == 1.0 / 60.0));
        }
        // At 165 Hz most frames run none.
        let (_, plans) = run(1.0 / 165.0, 165);
        assert!(plans.iter().filter(|p| p.steps == 0).count() > 90);
    }

    #[test]
    fn a_slow_frame_runs_a_bounded_number_of_steps() {
        let mut f = FixedStep::new();
        f.set_hz(60.0);
        assert_eq!(f.plan(1.0, 1.0).steps, MAX_STEPS_PER_FRAME);
        assert!(f.alpha() < 1e-3, "the time beyond the bound is dropped, not owed");
    }

    /// Slow motion and a hitstop shorten each step; they do not skip steps,
    /// so a script still runs and can count real time.
    #[test]
    fn time_scale_shortens_steps() {
        let mut f = FixedStep::new();
        f.set_hz(50.0);
        let p = f.plan(0.04, 0.0);
        assert_eq!((p.steps, p.dt), (2, 0.0));
        let p = f.plan(0.02, 0.5);
        assert_eq!((p.steps, p.dt), (1, 0.01));
    }

    #[test]
    fn what_moved_is_drawn_between_steps_and_put_back() {
        let mut world = World::new();
        let mover = world.spawn((at(0.0),));
        let still = world.spawn((at(5.0),));
        let mut f = FixedStep::new();
        f.set_hz(60.0);
        f.plan(1.0 / 60.0 + 1.0 / 240.0, 1.0); // one step, a quarter into the next
        f.begin_step(&world);
        world.get::<&mut Transform>(mover).unwrap().position.x = 10.0;
        let born = world.spawn((at(7.0),));
        f.blend_for_draw(&mut world);
        let x = |w: &World, e| w.get::<&Transform>(e).unwrap().position.x;
        assert!((x(&world, mover) - 2.5).abs() < 1e-3, "{}", x(&world, mover));
        assert_eq!(x(&world, still), 5.0);
        assert_eq!(x(&world, born), 7.0, "a thing born in the step is drawn where it is");
        f.restore(&mut world);
        assert_eq!(x(&world, mover), 10.0);
    }

    #[test]
    fn a_write_after_the_blend_stands() {
        let mut world = World::new();
        let bone = world.spawn((at(0.0),));
        let mut f = FixedStep::new();
        f.set_hz(60.0);
        f.plan(1.5 / 60.0, 1.0);
        f.begin_step(&world);
        world.get::<&mut Transform>(bone).unwrap().position.x = 4.0;
        f.blend_for_draw(&mut world);
        // A skeleton poses the bone for drawing after the blend.
        world.get::<&mut Transform>(bone).unwrap().position.x = 9.0;
        f.restore(&mut world);
        assert_eq!(world.get::<&Transform>(bone).unwrap().position.x, 9.0);
    }

    #[test]
    fn a_scene_can_choose_its_own_rate() {
        let mut f = FixedStep::new();
        f.set_hz(60.0);
        f.enter_scene(scene_simulation_hz(&serde_json::json!({ "name": "menu", "simulation_hz": 0 })));
        assert!(!f.is_on(), "a scene can turn it off");
        f.enter_scene(scene_simulation_hz(&serde_json::json!({ "name": "level" })));
        assert_eq!(f.hz(), 60.0, "a scene that says nothing has the project's");
        f.set_hz(0.0);
        f.enter_scene(Some(30.0));
        assert_eq!(f.hz(), 30.0, "a scene can turn it on");
        f.set_hz(120.0);
        assert_eq!(f.hz(), 30.0, "and keeps its own when the project's changes");
    }

    #[test]
    fn a_gate_holds_the_steps_until_it_opens() {
        let mut f = FixedStep::new();
        f.set_hz(60.0);
        f.reset();
        set_gate(Some(2));
        let ran: u32 = (0..20).map(|_| f.plan(1.0 / 60.0, 1.0).steps).sum();
        assert_eq!(ran, 2, "no step past the gate");
        // The time waited is kept, a frame's worth of steps at most.
        set_gate(Some(100));
        assert_eq!(f.plan(0.0, 1.0).steps, MAX_STEPS_PER_FRAME);
        f.reset();
        assert_eq!(gate(), None, "a new scene lifts the gate");
    }

    #[test]
    fn a_machine_behind_the_gate_catches_up_fast() {
        let mut f = FixedStep::new();
        f.set_hz(60.0);
        f.reset();
        set_gate(Some(500));
        set_catch_up(50);
        let mut frames = 0;
        while f.count() < 500 {
            f.plan(1.0 / 60.0, 1.0);
            frames += 1;
        }
        assert_eq!(f.count(), 500, "never past the gate");
        assert!(frames <= 10, "500 steps in {frames} frames");
        set_catch_up(0);
        set_gate(Some(1000));
        assert_eq!(f.plan(1.0 / 60.0, 1.0).steps, 1, "and back to real time");
    }

    #[test]
    fn the_world_hash_reads_roots_in_id_order() {
        let mut a = World::new();
        let root = a.spawn((at(1.0),));
        a.spawn((at(2.0), super::super::components::Parent(root)));
        let h = world_hash(&a);
        // A bone posed differently for drawing does not change it.
        for (_, t) in a.query_mut::<(&mut Transform, &super::super::components::Parent)>() {
            t.0.position.x = 99.0;
        }
        assert_eq!(world_hash(&a), h);
        a.get::<&mut Transform>(root).unwrap().position.x = 1.5;
        assert_ne!(world_hash(&a), h);
    }

    #[test]
    fn rotation_turns_the_short_way() {
        let a = Transform { rotation: 3.0, ..Default::default() };
        let b = Transform { rotation: -3.0, ..Default::default() };
        let mid = lerp_transform(&a, &b, 0.5);
        assert!((mid.rotation - (3.0 + (std::f32::consts::TAU - 6.0) / 2.0)).abs() < 1e-5, "{}", mid.rotation);
    }
}
