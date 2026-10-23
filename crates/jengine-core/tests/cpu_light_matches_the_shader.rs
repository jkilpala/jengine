//! The CPU light sampler and the WGSL that actually lights the screen must
//! agree.
//!
//! # Why this is worth a test of its own
//!
//! `renderer::lighting::light_at` answers "how much light reaches this world
//! point" on the CPU, so a game can ask whether a player is visible, whether a
//! corner is dark enough to hide in, whether a plant has had any light. The
//! renderer answers the same question per pixel, in `point_light.wgsl`.
//!
//! **Two implementations of one rule** is the failure this repo keeps meeting -
//! a bomb game's bot that would have died to explosions it was sure
//! were safe, the physics handoff in BUGS #144 where neither end existed. Here
//! it is worse than usual, because a disagreement is *invisible*: the screen
//! shows a lit corridor and the simulation calls it dark, or the reverse, and a
//! player reports that the game feels unfair rather than that it is broken.
//!
//! So the CPU version is a deliberate transcription of the shader, and this
//! file pins the constants they share. It cannot prove the two compute the same
//! thing - that would need a GPU and a readback, which `render_golden` does for
//! pixels - but it does fail the moment somebody edits one side's numbers
//! without the other, which is how they would actually drift.

use jengine_core::ecs::components::{AmbientLight, PointLight2D, SpotLight2D};
use jengine_core::renderer::lighting::{
    self, LightData, OCCLUDER_POOL, RAY_EPS_FAR, RAY_EPS_NEAR, SHADOW_SAMPLES, SPOT_EDGE,
};

fn shader(name: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/shaders/wgsl")
        .join(name);
    let src = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("reading {}: {e}", p.display()));
    // The two lights are compiled with the shared shadow code appended.
    if name == "point_light.wgsl" || name == "spot_light.wgsl" {
        src + &shader("light_shadow.wgsl")
    } else {
        src
    }
}

// ------------------------------------------------- the constants they share

/// The occluder pool is declared in three places: this constant, the wgpu
/// backend (which defers to it), and the WGSL array size. The shader is the
/// one that cannot import it.
#[test]
fn the_occluder_pool_matches_the_shader() {
    for name in ["point_light.wgsl", "spot_light.wgsl"] {
        let src = shader(name);
        let want = format!("const OCCLUDER_POOL: u32 = {OCCLUDER_POOL}u;");
        assert!(
            src.contains(&want),
            "{name} does not declare `{want}` - the CPU sampler would pack a different pool \
             from the screen's"
        );
        // And the array it sizes.
        let arr = format!("array<vec4<f32>, {OCCLUDER_POOL}>");
        assert!(
            src.contains(&arr),
            "{name}: the segment array is not `{arr}`, so the declared cap and the real one \
             differ"
        );
    }
}

/// The ray epsilons decide whether a segment touching a wall's own corner
/// shadows it. Off by one of these and a character standing against a wall is
/// in shadow on one side of the system and lit on the other.
#[test]
fn the_ray_epsilons_match_the_shader() {
    for name in ["point_light.wgsl", "spot_light.wgsl"] {
        let src = shader(name);
        let want = format!("t > ray_eps_near(length(r)) && t < {RAY_EPS_FAR}");
        assert!(
            src.contains(&want),
            "{name} no longer says `{want}`; the CPU sampler's blocking rule has drifted \
             from the shader's"
        );
        // The near epsilon: a thousandth of the ray, or half a unit on a long
        // one (`lighting::ray_eps_near`) - a sun's ray is 40,000 units.
        let eps = format!("return min({RAY_EPS_NEAR}, {} / max(len, 1e-6));", lighting::RAY_EPS_NEAR_UNITS);
        assert!(src.contains(&eps), "{name} no longer says `{eps}`");
    }
}

#[test]
fn the_soft_shadow_sample_count_matches_the_shader() {
    let src = shader("point_light.wgsl");
    let want = format!("const SHADOW_SAMPLES: i32 = {SHADOW_SAMPLES};");
    assert!(src.contains(&want), "point_light.wgsl no longer says `{want}`");
}

/// Whether an occluder shadows its own inside (`self_shadow`) is decided by
/// parity along the ray, and the light's own containment - the CPU's
/// `ray_transmit` transcribes these lines in both light shaders.
#[test]
fn the_self_shadow_rule_matches_the_shader() {
    for file in ["point_light.wgsl", "spot_light.wgsl"] {
        let src = shader(file);
        let flag = format!("const NO_SELF_SHADOW: u32 = 0x{:08x}u;", lighting::NO_SELF_SHADOW);
        for line in [
            flag.as_str(),
            "return 1u << (o & 31u);",
            "parity = parity ^ owner_bit(o);",
            "if ((o & NO_SELF_SHADOW) != 0u && ray_crosses(frag, light, seg.xy, seg.zw)) {",
            "let side_a = r.x * (a.y - p.y) - r.y * (a.x - p.x) >= 0.0;",
            // `shadow_length`, packed into the owner word as the CPU packs it.
            // ... stretched by the light's shadow scale (a sun's `1 / height`).
            "let len = f32((o >> 20u) & 0x7FFu) * 4.0 * u_light.extra.z;",
            "return 1.0 - smoothstep(len * 0.75, len, near);",
            "near[o & 31u] = min(near[o & 31u], crossing_distance(frag, light, seg.xy, seg.zw));",
            "reach = shadow_reach(o, near[o & 31u]);",
            "transmit = transmit * mix(vec3<f32>(1.0), occluder_transmit(i), reach);",
            "let inside = parity ^ u_light.occ.z;",
            "if ((o & NO_SELF_SHADOW) != 0u && (inside & owner_bit(o)) != 0u) {",
        ] {
            assert!(
                src.contains(line),
                "{file} no longer contains `{line}` - `lighting::ray_transmit` transcribes exactly \
                 this, so a point inside an occluder is lit on one side of the system and dark on \
                 the other"
            );
        }
    }
}

/// The falloff curve, which is the whole shape of a light.
#[test]
fn the_attenuation_curve_matches_the_shader() {
    let src = shader("point_light.wgsl");
    for line in [
        "var attenuation = 1.0 - clamp(dist / radius, 0.0, 1.0);",
        "attenuation     = pow(attenuation, falloff);",
        // `bump` is the normal buffer's answer (Sep 2026): 1.0 exactly for a
        // flat surface - see `a_flat_surface_takes_the_whole_light` - which
        // is every surface `point_light_at` models. `through` is the cookie
        // (`the_cookie_matches_the_shader`), which `light_summed` multiplies
        // in as here. The highlight (`light_specular.wgsl`) goes to its own
        // buffer and is zero for every surface `light_at` models.
        // `on_light_layers` is 1 for every light `light_summed` keeps: it
        // skips a light whose `light_mask` leaves the asked layers out
        // (`light_at_on`), as this zeroes it.
        "let lit = color * intensity * attenuation * shadow * through * on_light_layers(in.clip_position);",
        "out.light = vec4<f32>(lit * bump, 1.0);",
    ] {
        assert!(
            src.contains(line),
            "point_light.wgsl no longer contains `{line}` - `point_light_at` transcribes exactly \
             this, so it has drifted"
        );
    }
}

/// The light buffer is **cleared to ambient** and lights add into it, which is
/// why `light_at` starts from ambient rather than multiplying by it.
#[test]
fn the_composite_multiplies_and_ambient_is_the_clear() {
    let comp = shader("light_composite.wgsl");
    assert!(
        comp.contains("scene.rgb * light"),
        "the composite no longer multiplies; `light_at` returns the multiplier and would be \
         the wrong quantity"
    );
    let backend = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/renderer/backend/wgpu/lighting.rs"),
    )
    .unwrap();
    assert!(
        backend.contains("r: (amb.color[0] * amb.intensity) as f64"),
        "the light buffer is no longer cleared to `color * intensity`, so `light_at`'s ambient \
         term is wrong"
    );
}

// ------------------------------------------------------ the sampler itself

fn one_light(pos: glam::Vec2, radius: f32) -> LightData {
    LightData {
        point_lights: vec![(pos, PointLight2D { radius, falloff: 1.0, ..Default::default() })],
        ambient: AmbientLight { color: [1.0, 1.0, 1.0], intensity: 0.0 },
        ..Default::default()
    }
}

#[test]
fn light_falls_off_to_nothing_at_the_radius() {
    let l = one_light(glam::Vec2::ZERO, 100.0);
    let at = |d: f32| lighting::brightness_at(glam::Vec2::new(d, 0.0), &l);

    assert!((at(0.0) - 1.0).abs() < 1e-5, "full brightness at the centre: {}", at(0.0));
    assert!((at(50.0) - 0.5).abs() < 1e-5, "linear falloff halfway: {}", at(50.0));
    assert_eq!(at(100.0), 0.0, "nothing at the edge");
    assert_eq!(at(180.0), 0.0, "and nothing beyond it");
}

#[test]
fn the_falloff_exponent_bends_the_curve() {
    let mut l = one_light(glam::Vec2::ZERO, 100.0);
    l.point_lights[0].1.falloff = 2.0;
    // Quadratic: half way out is a quarter as bright, not half.
    let half = lighting::brightness_at(glam::Vec2::new(50.0, 0.0), &l);
    assert!((half - 0.25).abs() < 1e-5, "{half}");
}

#[test]
fn ambient_is_the_floor_everywhere() {
    let mut l = one_light(glam::Vec2::ZERO, 100.0);
    l.ambient = AmbientLight { color: [1.0, 1.0, 1.0], intensity: 0.2 };
    // Far outside the light, only ambient is left.
    let far = lighting::brightness_at(glam::Vec2::new(900.0, 0.0), &l);
    assert!((far - 0.2).abs() < 1e-5, "{far}");
    // And it adds to the light rather than replacing it.
    let centre = lighting::brightness_at(glam::Vec2::ZERO, &l);
    assert!((centre - 1.2).abs() < 1e-5, "{centre}");
}

/// **The thing a dark game is built on.** A wall between you and the lamp
/// leaves you in shadow.
#[test]
fn an_occluder_between_the_point_and_the_light_casts_a_shadow() {
    let mut l = one_light(glam::Vec2::ZERO, 400.0);
    // A vertical wall at x = 50, spanning the ray to a point at x = 100.
    l.occluders = vec![[50.0, -40.0, 50.0, 40.0]];
    l.occluder_tints = vec![[0.0, 0.0, 0.0, 1.0]];

    let behind = lighting::brightness_at(glam::Vec2::new(100.0, 0.0), &l);
    assert_eq!(behind, 0.0, "fully shadowed behind an opaque wall");

    // Beside the wall, and clear of it. Note how little clearance that takes:
    // at (100, 80) the ray crosses x = 50 at exactly y = 40, the wall's own
    // endpoint, and `u <= 1.0` counts that as blocking - so the corner of a
    // wall shadows precisely, with no rounding in the player's favour.
    let beside = lighting::brightness_at(glam::Vec2::new(100.0, 160.0), &l);
    assert!(beside > 0.1, "clear line of sight is lit: {beside}");

    // The graze itself, pinned so the behaviour is deliberate rather than
    // discovered by somebody standing in a doorway.
    let graze = lighting::brightness_at(glam::Vec2::new(100.0, 80.0), &l);
    assert_eq!(graze, 0.0, "a ray through the wall's endpoint is blocked");
}

/// A translucent or coloured occluder passes some light, which is what
/// `occluder_transmit` is for - and the only way a stained window or a curtain
/// differs from a wall.
#[test]
fn a_translucent_occluder_passes_some_light() {
    let mut l = one_light(glam::Vec2::ZERO, 200.0);
    l.occluders = vec![[50.0, -40.0, 50.0, 40.0]];
    l.occluder_tints = vec![[1.0, 0.0, 0.0, 0.5]]; // half-opaque red

    let rgb = lighting::light_at(glam::Vec2::new(100.0, 0.0), &l);
    assert!(rgb.x > 0.0, "red gets through: {rgb:?}");
    assert!(rgb.y < rgb.x, "and less green than red does");
}

/// Edges out of a light's reach spend nothing: a pool's worth of them far
/// away, and the one wall beside the light still casts its shadow.
///
/// Under the old global budget - the first sixty-four segments in the world,
/// for every light - this wall was number 449 and cast nothing.
#[test]
fn far_occluders_do_not_crowd_out_near_ones() {
    let mut l = one_light(glam::Vec2::ZERO, 400.0);
    for i in 0..OCCLUDER_POOL {
        let y = 1000.0 + i as f32;
        l.occluders.push([-10.0, y, 10.0, y]);
        l.occluder_tints.push([0.0, 0.0, 0.0, 1.0]);
    }
    l.occluders.push([50.0, -40.0, 50.0, 40.0]);
    l.occluder_tints.push([0.0, 0.0, 0.0, 1.0]);

    let packed = lighting::pack_light_occluders(&l);
    assert_eq!(packed.point, vec![(0, 1)], "only the near wall is in the light's reach");
    let lit = lighting::brightness_at(glam::Vec2::new(100.0, 0.0), &l);
    assert!(lit < 0.05, "the wall beside the light shadows the point behind it: {lit}");
}

/// A full pool keeps each light's **nearest** edges: the farthest are the
/// ones that go, and the CPU drops exactly the ones the GPU drops.
#[test]
fn a_full_pool_drops_the_farthest_edges() {
    let mut l = one_light(glam::Vec2::ZERO, 400.0);
    // A pool's worth of short edges close to the light, off the ray...
    for i in 0..OCCLUDER_POOL {
        let y = 20.0 + (i % 10) as f32;
        let x = -30.0 - (i / 10) as f32;
        l.occluders.push([x, y, x - 1.0, y]);
        l.occluder_tints.push([0.0, 0.0, 0.0, 1.0]);
    }
    // ...and a wall further out that would block the ray.
    l.occluders.push([150.0, -40.0, 150.0, 40.0]);
    l.occluder_tints.push([0.0, 0.0, 0.0, 1.0]);

    let packed = lighting::pack_light_occluders(&l);
    assert_eq!(packed.point, vec![(0, OCCLUDER_POOL)]);
    assert_eq!(packed.dropped, 1, "the far wall did not fit");
    let lit = lighting::brightness_at(glam::Vec2::new(200.0, 0.0), &l);
    assert!(lit > 0.05, "a dropped edge casts no shadow on the CPU either: {lit}");
}

/// Many lights over little geometry share one list: sixty lights along a
/// sixteen-edge wall would be 960 per-light entries, but the wall is sixteen
/// edges, so every light is handed those sixteen and nothing is dropped.
#[test]
fn many_lights_over_little_geometry_share_one_list() {
    let mut l = one_light(glam::Vec2::ZERO, 200.0);
    l.point_lights.clear();
    for i in 0..60 {
        l.point_lights.push((
            glam::Vec2::new(-300.0 + i as f32 * 10.0, -60.0),
            PointLight2D { radius: 200.0, falloff: 1.0, ..Default::default() },
        ));
    }
    for i in 0..4 {
        // Four boxes along y = 0: sixteen edges.
        let x = -200.0 + i as f32 * 120.0;
        for s in [[x, -5.0, x + 60.0, -5.0], [x + 60.0, -5.0, x + 60.0, 5.0], [x + 60.0, 5.0, x, 5.0], [x, 5.0, x, -5.0]] {
            l.occluders.push(s);
            l.occluder_tints.push([0.0, 0.0, 0.0, 1.0]);
        }
    }
    let packed = lighting::pack_light_occluders(&l);
    assert_eq!(packed.dropped, 0);
    assert_eq!(packed.segments.len(), 16, "one shared copy of the wall");
    assert!(packed.point.iter().all(|r| *r == (0, 16) || *r == (0, 0)));
    // And the answer is what testing every edge for every light gives.
    let behind = glam::Vec2::new(-170.0, 40.0);
    let every_edge: glam::Vec3 = l
        .point_lights
        .iter()
        .map(|(pos, light)| lighting::point_light_at(behind, *pos, light, lighting::Occluders::plain(&l.occluders, &l.occluder_tints)))
        .sum();
    let packed_answer = lighting::light_at(behind, &l);
    assert!((packed_answer - every_edge).length() < 1e-5, "{packed_answer} vs {every_edge}");
    assert!(every_edge.x > 0.0, "the point is lit by some light, so the comparison means something");
}

#[test]
fn a_segment_touching_the_endpoints_does_not_block() {
    // A wall whose line passes exactly through the light: `t` lands outside
    // the epsilons, so it must not count.
    let mut l = one_light(glam::Vec2::ZERO, 200.0);
    l.occluders = vec![[0.0, -40.0, 0.0, 40.0]];
    l.occluder_tints = vec![[0.0, 0.0, 0.0, 1.0]];
    assert!(lighting::brightness_at(glam::Vec2::new(100.0, 0.0), &l) > 0.1);
}

#[test]
fn no_lights_at_all_leaves_only_ambient() {
    let l = LightData {
        ambient: AmbientLight { color: [0.5, 0.5, 0.5], intensity: 0.1 },
        ..Default::default()
    };
    assert!((lighting::brightness_at(glam::Vec2::new(12.0, 34.0), &l) - 0.05).abs() < 1e-5);
}

/// The CPU models a flat world. That stays true for the GPU only while a
/// flat normal takes a light's whole contribution: `normal_response`
/// divides N.L by `l.z`, which is N.L for n = (0, 0, 1), so the ratio is 1.
/// Drop the division and every light in every scene without normal maps
/// dims to ~45% - and `light_at` no longer answers for the screen.
#[test]
fn a_flat_surface_takes_the_whole_light() {
    for name in ["point_light.wgsl", "spot_light.wgsl"] {
        let src = shader(name);
        assert!(
            src.contains("return min(max(dot(n, l), 0.0) / l.z, u_light.extra.y);"),
            "{name}: `normal_response` is no longer relative to a flat surface"
        );
    }
    // The cap on a face turned to the light must not cut a flat one.
    assert!(jengine_core::ecs::components::MAX_NORMAL_RESPONSE >= 1.0);
}

/// A light's cookie: where its picture lies and what is outside it.
/// `lighting::cookie_factor` transcribes `light_cookie.wgsl`'s
/// `cookie_factor`, and both lights call it with their turn - a point
/// light's `spot.x`, a spot's aim.
#[test]
fn the_cookie_matches_the_shader() {
    let src = shader("light_cookie.wgsl");
    for line in [
        "let local = vec2<f32>(d.x * c + d.y * s, -d.x * s + d.y * c) / reach;",
        "if (abs(local.x) > 1.0 || abs(local.y) > 1.0) {",
        "vec2<f32>(local.x * 0.5 + 0.5, 0.5 - local.y * 0.5)",
        "return t.rgb * t.a;",
    ] {
        assert!(
            src.contains(line),
            "light_cookie.wgsl no longer contains `{line}` - `cookie_factor` transcribes exactly this"
        );
    }
    assert!(shader("point_light.wgsl").contains("cookie_factor(in.world_pos, center, u_light.spot.x, u_light.extra.w)"));
    assert!(shader("spot_light.wgsl").contains("cookie_factor(in.world_pos, center, spot_dir, u_light.extra.w)"));
    let cpu = include_str!("../src/renderer/lighting.rs");
    for line in [
        "let local = glam::Vec2::new(d.x * c + d.y * s, -d.x * s + d.y * c) / reach.max(1e-4);",
        "if local.x.abs() > 1.0 || local.y.abs() > 1.0 {",
        "glam::Vec2::new(local.x * 0.5 + 0.5, 0.5 - local.y * 0.5)",
    ] {
        assert!(cpu.contains(line), "`cookie_factor` no longer contains `{line}`");
    }
}

// ------------------------------------------------------------- spot lights

/// The cone, which is the whole shape of a spot light, and the terms it is
/// multiplied into. `spot_light_at` transcribes exactly these lines.
#[test]
fn the_spot_cone_matches_the_shader() {
    let src = shader("spot_light.wgsl");
    for line in [
        "let dir       = vec2<f32>(cos(spot_dir), sin(spot_dir));",
        "let cos_cone  = cos(spot_ang);",
        "var cone      = smoothstep(cos_cone - 0.05, cos_cone + 0.05, cos_frag);",
        "let lit = color * intensity * attenuation * cone * shadow * through * on_light_layers(in.clip_position);",
        "out.light = vec4<f32>(lit * bump, 1.0);",
    ] {
        assert!(
            src.contains(line),
            "spot_light.wgsl no longer contains `{line}` - `spot_light_at` transcribes exactly \
             this, so it has drifted"
        );
    }
    // And the cone-edge width as a named constant on this side.
    assert!(
        src.contains(&format!("cos_cone - {SPOT_EDGE}, cos_cone + {SPOT_EDGE}")),
        "the shader's cone edge is no longer `{SPOT_EDGE}`"
    );
}

/// **The direction is the entity's rotation plus the component's**, and in
/// that order. The wgpu pass builds `spot.x` that way; a sampler that used
/// only the component field would answer for a searchlight that never turns.
#[test]
fn the_spot_direction_is_built_the_same_way_as_the_uniform() {
    let backend = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/renderer/backend/wgpu/lighting.rs"),
    )
    .unwrap();
    assert!(
        backend.contains("let dir = entity_rotation + light.direction;"),
        "the wgpu pass no longer sums the entity rotation and the component direction, so \
         `spot_light_at`'s cone points somewhere the screen's does not"
    );
}

fn one_spot(angle: f32, direction: f32) -> LightData {
    LightData {
        spot_lights: vec![(
            glam::Vec2::ZERO,
            0.0,
            SpotLight2D {
                radius: 200.0,
                falloff: 1.0,
                angle,
                direction,
                color: [1.0, 1.0, 1.0],
                intensity: 1.0,
                shadow_softness: 0.0,
                height: jengine_core::ecs::components::DEFAULT_LIGHT_HEIGHT,
                casts_shadows: true,
                ..Default::default()
            },
        )],
        ambient: AmbientLight { color: [1.0, 1.0, 1.0], intensity: 0.0 },
        ..Default::default()
    }
}

#[test]
fn a_spot_lights_along_its_direction_and_not_behind_it() {
    // A 30-degree half-angle cone pointing right (+x).
    let l = one_spot(std::f32::consts::FRAC_PI_6, 0.0);
    let ahead = lighting::brightness_at(glam::Vec2::new(100.0, 0.0), &l);
    let behind = lighting::brightness_at(glam::Vec2::new(-100.0, 0.0), &l);
    let beside = lighting::brightness_at(glam::Vec2::new(0.0, 100.0), &l);
    assert!(ahead > 0.4, "straight down the cone should be lit, got {ahead}");
    assert!(behind < 1e-6, "behind the spot should be black, got {behind}");
    assert!(beside < 1e-6, "90 degrees off a 30-degree cone should be black, got {beside}");
}

/// The entity's rotation turns the cone. This is the half of the direction
/// that a scene authors by rotating the light, and the half a sampler reading
/// only `SpotLight2D::direction` would miss.
#[test]
fn the_entity_rotation_turns_the_cone() {
    let mut l = one_spot(std::f32::consts::FRAC_PI_6, 0.0);
    let up = glam::Vec2::new(0.0, 100.0);
    assert!(lighting::brightness_at(up, &l) < 1e-6, "unrotated, up is outside the cone");
    l.spot_lights[0].1 = std::f32::consts::FRAC_PI_2; // turn the entity a quarter turn
    assert!(
        lighting::brightness_at(up, &l) > 0.4,
        "turning the entity a quarter turn should swing the cone onto +y"
    );
}

/// The edge is a `smoothstep`, not a hard cut - so the rim of the cone is
/// partly lit. A game thresholding "is this attacker revealed" needs the same
/// gradient the player sees, or the edge of the beam lies.
#[test]
fn the_cone_edge_is_soft() {
    let half = std::f32::consts::FRAC_PI_6;
    let l = one_spot(half, 0.0);
    // Exactly on the geometric edge, smoothstep is at its midpoint.
    let on_edge = glam::Vec2::new(100.0 * half.cos(), 100.0 * half.sin());
    let b = lighting::brightness_at(on_edge, &l);
    let centre = lighting::brightness_at(glam::Vec2::new(100.0, 0.0), &l);
    assert!(b > 0.01 && b < centre, "the cone edge should be partly lit: {b} vs centre {centre}");
}

/// Spots obey the same occluders as points - the shader shares the whole
/// `shadow_factor` and the one occluder uniform.
#[test]
fn a_spot_is_blocked_by_an_occluder() {
    let mut l = one_spot(std::f32::consts::FRAC_PI_3, 0.0);
    let p = glam::Vec2::new(120.0, 0.0);
    let lit = lighting::brightness_at(p, &l);
    l.occluders = vec![[60.0, -40.0, 60.0, 40.0]];
    l.occluder_tints = vec![[0.0, 0.0, 0.0, 1.0]];
    let shadowed = lighting::brightness_at(p, &l);
    assert!(lit > 0.3, "unobstructed it should be lit, got {lit}");
    assert!(shadowed < 1e-6, "a wall across the beam should black it out, got {shadowed}");
}

/// And the whole point: `light_at` now sums both kinds. A game asking "is this
/// point dark" gets one answer covering every light on screen.
#[test]
fn light_at_sums_point_and_spot_lights_together() {
    let mut l = one_spot(std::f32::consts::FRAC_PI_3, 0.0);
    let p = glam::Vec2::new(100.0, 0.0);
    let spot_only = lighting::brightness_at(p, &l);
    assert!(spot_only > 0.3, "the spot alone must light the point, got {spot_only}");
    l.point_lights =
        vec![(glam::Vec2::new(100.0, 0.0), PointLight2D { radius: 50.0, falloff: 1.0, ..Default::default() })];
    let both = lighting::brightness_at(p, &l);
    assert!(
        both > spot_only + 0.1,
        "a point light on top of the spot must add: {both} vs {spot_only}"
    );
}
