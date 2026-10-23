//! **Noise for scripts** (`Noise.*`, Oct 2026; `jengine_core::noise`):
//! Perlin in two and three dimensions, simplex, cellular (Worley) and
//! fractal sums of them, plus a whole grid in one call. Before it a script
//! that wanted a smooth random field wrote value noise in Lua by hand - the
//! showcase's Mapmaker did - and a game's islands depended on which runtime
//! drew them. Every runtime calls the same Rust now: native through Luau
//! globals, the browsers through one `state.noise` hook. Found by the
//! showcase's Procedural Isles.

use jengine_core::noise::{self, Fractal, Kind, Mode};

/// A spread of sample points, not on the lattice.
fn points() -> impl Iterator<Item = (f64, f64)> {
    (0..4000).map(|i| {
        let i = i as f64;
        ((i * 0.618_034).fract() * 40.0 - 20.0 + 0.013, (i * 0.414_214).fract() * 40.0 - 20.0 + 0.007)
    })
}

#[test]
fn each_noise_is_smooth_bounded_and_spread() {
    let fields: [(&str, Box<dyn Fn(f64, f64) -> f64>); 3] = [
        ("perlin", Box::new(|x, y| noise::perlin(x, y, 3))),
        ("perlin3", Box::new(|x, y| noise::perlin3(x, y, 0.37, 3))),
        ("simplex", Box::new(|x, y| noise::simplex(x, y, 3))),
    ];
    for (name, f) in fields {
        let (mut lo, mut hi, mut sum, mut n) = (f64::MAX, f64::MIN, 0.0, 0.0);
        for (x, y) in points() {
            let v = f(x, y);
            assert!((-1.0..=1.0).contains(&v), "{name} at ({x}, {y}) is {v}");
            // Smooth: a thousandth of a cell away is nearly the same.
            assert!((f(x + 0.001, y) - v).abs() < 0.01, "{name} jumps at ({x}, {y})");
            lo = lo.min(v);
            hi = hi.max(v);
            sum += v;
            n += 1.0;
        }
        assert!(lo < -0.55 && hi > 0.55, "{name} uses its range: {lo}..{hi}");
        assert!((sum / n).abs() < 0.08, "{name} is centred: mean {}", sum / n);
    }
}

#[test]
fn a_seed_is_a_different_field_and_the_same_seed_the_same_one() {
    let (x, y) = (3.3, -1.7);
    assert_eq!(noise::simplex(x, y, 11), noise::simplex(x, y, 11));
    assert_ne!(noise::simplex(x, y, 11), noise::simplex(x, y, 12));
    assert_ne!(noise::perlin(x, y, 0), noise::perlin(x, y, 1));
    assert_eq!(noise::cellular(x, y, 5), noise::cellular(x, y, 5));
    assert_ne!(noise::cellular(x, y, 5), noise::cellular(x, y, 6));
    // Whole numbers from a script are seeds as they stand.
    assert_eq!(noise::seed_from(42.0), 42);
}

#[test]
fn cellular_measures_the_nearest_two_points() {
    let mut values = Vec::new();
    for (x, y) in points() {
        let c = noise::cellular(x, y, 9);
        assert!(c.f1 <= c.f2, "f1 {} past f2 {}", c.f1, c.f2);
        assert!(c.f1 < 1.5, "a feature point in every cell keeps f1 under 1.5: {}", c.f1);
        assert!((0.0..1.0).contains(&c.value));
        values.push(c.value);
    }
    // A cell's value holds across it: a step far smaller than a cell
    // changes it only where it crosses a border, which is rare.
    let changed = points()
        .filter(|&(x, y)| noise::cellular(x, y, 9).value != noise::cellular(x + 0.002, y, 9).value)
        .count();
    assert!(changed < 40, "{changed} of 4000 tiny steps changed cell");
    values.sort_by(f64::total_cmp);
    values.dedup();
    assert!(values.len() > 500, "regions get values of their own: {}", values.len());
}

#[test]
fn fractals_sum_octaves_in_their_ranges() {
    for kind in [Kind::Perlin, Kind::Perlin3, Kind::Simplex, Kind::Cellular] {
        for (mode, range) in [(Mode::Fbm, -1.0..=1.0), (Mode::Ridged, 0.0..=1.0), (Mode::Turbulence, 0.0..=1.0)] {
            let f = Fractal { kind, mode, z: 0.4, ..Default::default() };
            for (x, y) in points().take(500) {
                let v = f.sample(x, y);
                assert!(range.contains(&v), "{kind:?} {mode:?} at ({x}, {y}) is {v}");
            }
        }
    }
    // More octaves is more fine detail: the change over a hundredth of a
    // cell, against the change over a whole one, grows. (The raw change does
    // not - a normalised sum turns its first octave down to make room.)
    let rough = |octaves| {
        let f = Fractal { octaves, ..Default::default() };
        let step = |d: f64| points().map(|(x, y)| (f.sample(x + d, y) - f.sample(x, y)).abs()).sum::<f64>();
        step(0.01) / step(1.0)
    };
    assert!(rough(6) > rough(1) * 1.3, "six octaves {} against one {}", rough(6), rough(1));
    // `frequency` scales the input.
    let f = Fractal { frequency: 0.25, ..Default::default() };
    assert_eq!(f.sample(8.0, 4.0), Fractal::default().sample(2.0, 1.0));
}

#[test]
fn a_grid_is_its_samples_row_by_row() {
    let f = Fractal { kind: Kind::Perlin, mode: Mode::Ridged, seed: 4, ..Default::default() };
    let g = f.grid(1.5, -2.0, 7, 3, 0.25);
    assert_eq!(g.len(), 21);
    assert_eq!(g[0], f.sample(1.5, -2.0));
    assert_eq!(g[2 * 7 + 5], f.sample(1.5 + 5.0 * 0.25, -2.0 + 2.0 * 0.25));
    assert!(noise::script_grid(0.0, 0.0, 4096.0, 4096.0, 1.0, &serde_json::Value::Null).is_empty(), "too big is refused");
}

#[test]
fn a_script_table_reads_as_settings() {
    let f = Fractal::from_json(&serde_json::json!({
        "kind": "cellular", "mode": "turbulence", "octaves": 3, "lacunarity": 2.5,
        "gain": 0.4, "frequency": 0.05, "seed": 7, "z": 1.5
    }));
    assert_eq!(
        f,
        Fractal { kind: Kind::Cellular, mode: Mode::Turbulence, octaves: 3, lacunarity: 2.5, gain: 0.4, frequency: 0.05, seed: 7, z: 1.5 }
    );
    assert_eq!(Fractal::from_json(&serde_json::Value::Null), Fractal::default());
    assert_eq!(Fractal::from_json(&serde_json::json!({ "octaves": 99 })).octaves, 12);
}

/// Every op the browser preamble sends is one the hook answers.
#[test]
fn the_browser_hook_answers_every_op_the_preamble_sends() {
    let preamble = include_str!("../src/scripting/bridge/adapters/js/browser.rs");
    let hook = include_str!("../src/scripting/bridge/adapters/js/noise.rs");
    let ops: Vec<&str> = preamble.split("__noise('").skip(1).map(|s| s.split('\'').next().unwrap()).collect();
    assert_eq!(ops.len(), 6, "the scan found {ops:?}");
    for op in ops {
        assert!(hook.contains(&format!("\"{op}\" =>")), "the preamble sends '{op}' and the hook has no arm for it");
    }
}

#[cfg(feature = "native")]
#[test]
fn a_native_script_gets_the_numbers_rust_does() {
    use hecs::World;
    use jengine_core::ecs::components::{CustomComponents, Name, ScriptComponent, ScriptLanguage, Transform};
    use jengine_core::ecs::systems::ScriptSystem;
    use jengine_core::input::InputState;
    let src = r#"
function update(entity, dt)
    local f1, f2, v = Noise.cellular(2.5, 1.25, 3)
    local g = Noise.grid(0, 0, 4, 3, 0.5, { kind = "perlin", octaves = 2, seed = 8 })
    Entity.set_custom(entity, "Probe", "perlin", Noise.perlin(1.3, 2.7, 5))
    Entity.set_custom(entity, "Probe", "perlin3", Noise.perlin3(1.3, 2.7, 0.5))
    Entity.set_custom(entity, "Probe", "simplex", Noise.simplex(1.3, 2.7, 5))
    Entity.set_custom(entity, "Probe", "f1", f1)
    Entity.set_custom(entity, "Probe", "f2", f2)
    Entity.set_custom(entity, "Probe", "value", v)
    Entity.set_custom(entity, "Probe", "fractal", Noise.fractal(1.3, 2.7, { mode = "ridged", seed = 2 }))
    Entity.set_custom(entity, "Probe", "count", #g)
    Entity.set_custom(entity, "Probe", "last", g[12])
end
"#;
    let dir = std::env::temp_dir().join(format!("jengine_noise_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("noise.lua");
    std::fs::write(&path, src).unwrap();
    let mut world = World::new();
    let e = world.spawn((
        Name("Probe".into()),
        Transform::default(),
        CustomComponents(Default::default()),
        ScriptComponent { language: ScriptLanguage::Lua, source_path: path.to_string_lossy().into(), cull_distance: None },
    ));
    let mut system = ScriptSystem::new().expect("script system");
    let input = InputState::new();
    for _ in 0..2 {
        system.run(&mut world, 1.0 / 60.0, &input, None, None);
    }
    let c = world.get::<&CustomComponents>(e).unwrap();
    let probe = c.0.get("Probe").and_then(|p| p.as_object().cloned()).expect("the script wrote its probe");
    let read = |k: &str| probe.get(k).and_then(|v| v.as_f64()).unwrap_or_else(|| panic!("no {k} in {probe:?}"));
    let near = |k: &str, want: f64| assert!((read(k) - want).abs() < 1e-5, "{k}: {} against {want}", read(k));
    near("perlin", noise::perlin(1.3, 2.7, 5));
    near("perlin3", noise::perlin3(1.3, 2.7, 0.5, 0));
    near("simplex", noise::simplex(1.3, 2.7, 5));
    let cell = noise::cellular(2.5, 1.25, 3);
    near("f1", cell.f1);
    near("f2", cell.f2);
    near("value", cell.value);
    near("fractal", Fractal { mode: Mode::Ridged, seed: 2, ..Default::default() }.sample(1.3, 2.7));
    assert_eq!(read("count"), 12.0);
    let grid = Fractal { kind: Kind::Perlin, octaves: 2, seed: 8, ..Default::default() }.grid(0.0, 0.0, 4, 3, 0.5);
    near("last", grid[11]);
}
