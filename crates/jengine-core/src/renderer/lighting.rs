//! 2D lighting data — backend-agnostic light gather + ECS↔scene helpers.
//!
//! GPU lighting passes live in `renderer::backend::wgpu::lighting`. This
//! module only provides the data types and per-frame collection helpers
//! that both the runtime and the editor viewport feed into the wgpu pass.

use crate::detmath::DetMath;
use crate::ecs::components::{
    ALL_LIGHT_LAYERS, AmbientLight, AmbientOcclusion, DirectionalLight2D, LightOccluder, LightmapComponent, PointLight2D, SpotLight2D,
    TilemapOccluder, ToonLighting,
    SpriteComponent, Transform, WorldTransform,
};

/// Where a light, occluder or lightmap is: its resolved world transform when
/// it is in a hierarchy, else its own. Lights read `Transform` alone until
/// Sep 2026, so a light parented to a moving thing - a lantern held by a
/// character - stayed where its local offset put it, near the origin.
fn placed(t: &Transform, w: Option<&WorldTransform>) -> Transform {
    match w {
        Some(w) => Transform { position: w.position, rotation: w.rotation, scale: w.scale },
        None => t.clone(),
    }
}

/// A fog where it is: what the composite needs (`ecs::fog::placed`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedFog {
    pub fog_color: [f32; 4],
    /// noise scale, noise amount, ground, height.
    pub shape: [f32; 4],
    /// The drift so far (x, y), scatter, unused.
    pub motion: [f32; 4],
    /// The box's centre and half size (half size 0: everywhere).
    pub area: [f32; 4],
}

/// A freeform light placed in the world: its outline anticlockwise, its
/// colour times its intensity.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedFreeform {
    pub points: Vec<glam::Vec2>,
    pub color: glam::Vec3,
    pub falloff: f32,
}

/// Collected light data for a single frame, gathered from ECS.
#[derive(Clone)]
pub struct LightData {
    pub point_lights: Vec<(glam::Vec2, PointLight2D)>,
    pub spot_lights: Vec<(glam::Vec2, f32, SpotLight2D)>, // (pos, entity_rotation, light)
    /// Suns: `(entity_rotation, light)`. A sun has no position - it is placed
    /// far out along its direction when it is used ([`sun_as_point`]), and
    /// from then on it is a point light to every consumer.
    pub directional_lights: Vec<(f32, DirectionalLight2D)>,
    pub ambient: AmbientLight,
    /// World-space occluder edge segments `[x0, y0, x1, y1]`. Point and
    /// spot lights ray-test their line-of-sight against these to cast
    /// shadows. Built from [`LightOccluder`] components in
    /// [`gather_lights`].
    pub occluders: Vec<[f32; 4]>,
    /// Per-segment shadow tint `[r, g, b, opacity]`, parallel to
    /// [`Self::occluders`] (every edge of one occluder shares the same
    /// entry). The light transmitted through a blocking edge equals
    /// `mix([1,1,1], rgb, opacity)`, so `[0,0,0,1]` is an opaque neutral
    /// shadow and a coloured/partial entry yields translucent shadows.
    pub occluder_tints: Vec<[f32; 4]>,
    /// Per-segment owner, parallel to [`Self::occluders`]: which occluder the
    /// edge belongs to ([`OWNER_ID_MASK`]), its shadow's length
    /// ([`owner_shadow_length`]) and [`NO_SELF_SHADOW`] when that occluder
    /// does not shadow its own inside ([`LightOccluder::self_shadow`] off).
    /// Filled by [`Self::add_occluder`]; an edge with no entry (a caller
    /// that pushed segments itself) shadows everything, as all edges did
    /// before Sep 2026.
    pub occluder_owners: Vec<u32>,
    /// Baked lightmaps, where they sit. Read by [`light_at_textured`], which
    /// needs their pixels; [`light_at`] alone leaves them out.
    pub lightmaps: Vec<LightmapQuad>,
    /// Ambient occlusion, if the world asks for it ([`AmbientOcclusion`]):
    /// the ambient term darkened near the occluders' outlines.
    pub occlusion: Option<AmbientOcclusion>,
    /// Toon lighting, if the world asks for it ([`ToonLighting`]): the summed
    /// light stepped into bands ([`toon_step`]).
    pub toon: Option<ToonLighting>,
    /// Light that bounces, if the world asks for it
    /// ([`crate::ecs::components::GlobalIllumination`]): the GPU's only -
    /// [`light_at`] leaves it out.
    pub gi: Option<crate::ecs::components::GlobalIllumination>,
    /// Freeform lights, placed (`renderer::freeform_light`): drawn as meshes
    /// beside the lightmaps, and added by [`light_at`] as the mesh adds them.
    pub freeform: Vec<PlacedFreeform>,
    /// Fog that catches the light, if the world has some (`ecs::fog`): the
    /// composite's alone - [`light_at`] leaves it out.
    pub fog: Option<PlacedFog>,
}

/// One [`LightmapComponent`] placed in the world: what
/// `render_lightmaps_wgpu` draws into the light buffer.
#[derive(Debug, Clone)]
pub struct LightmapQuad {
    pub center: glam::Vec2,
    pub rotation: f32,
    /// World size, the transform's scale already applied.
    pub size: glam::Vec2,
    pub uv_rect: [f32; 4],
    pub intensity: f32,
    pub texture_id: String,
}

impl Default for LightData {
    fn default() -> Self {
        Self {
            point_lights: Vec::new(),
            spot_lights: Vec::new(),
            directional_lights: Vec::new(),
            ambient: AmbientLight {
                color: [1.0, 1.0, 1.0],
                intensity: 1.0, // no lighting by default (fully lit)
            },
            occluders: Vec::new(),
            occluder_tints: Vec::new(),
            occluder_owners: Vec::new(),
            lightmaps: Vec::new(),
            occlusion: None,
            toon: None,
            freeform: Vec::new(),
            gi: None,
            fog: None,
        }
    }
}

/// The bit of an occluder edge's owner (`LightData::occluder_owners`) that
/// says its occluder does not shadow its own inside.
pub const NO_SELF_SHADOW: u32 = 1 << 31;
/// The bits of an owner that are the occluder's index, of which the shadow
/// test uses the low five ([`owner_bit`]).
pub const OWNER_ID_MASK: u32 = (1 << 19) - 1;
/// Owner flag: this occluder's shadow skips pixels that are themselves
/// casters ([`LightOccluder::shadows_casters`] off). The GPU reads that from
/// the normal buffer; the CPU treats no point as a caster.
pub const NOT_ON_CASTERS: u32 = 1 << 19;
/// Where an owner keeps its shadow length: eleven bits from bit 20, in
/// [`SHADOW_LENGTH_STEP`]s, zero meaning unbounded. The uniform has no room
/// for another array per edge, and the owner word had eleven bits to spare.
pub const SHADOW_LENGTH_SHIFT: u32 = 20;
pub const SHADOW_LENGTH_STEP: f32 = 4.0;
const SHADOW_LENGTH_MAX_STEPS: u32 = 0x7FF;

/// An occluder's [`LightOccluder::shadow_length`] as the owner word carries
/// it: rounded to [`SHADOW_LENGTH_STEP`], at least one step when set.
pub fn shadow_length_bits(length: f32) -> u32 {
    if length <= 0.0 {
        return 0;
    }
    let steps = ((length / SHADOW_LENGTH_STEP).round() as u32).clamp(1, SHADOW_LENGTH_MAX_STEPS);
    steps << SHADOW_LENGTH_SHIFT
}

/// The shadow length an owner word carries; `0.0` is unbounded.
pub fn owner_shadow_length(owner: u32) -> f32 {
    ((owner >> SHADOW_LENGTH_SHIFT) & SHADOW_LENGTH_MAX_STEPS) as f32 * SHADOW_LENGTH_STEP
}

impl LightData {
    /// Add one occluder's edges, tints and owner - what [`gather_lights`]
    /// does for each enabled [`LightOccluder`]. A package building its own
    /// `LightData` should use this rather than [`append_occluder_segments`],
    /// or its occluders shadow their own inside while the screen's do not.
    pub fn add_occluder(&mut self, transform: &Transform, occ: &LightOccluder, sprite_size: Option<[f32; 2]>) {
        // Edges somebody pushed without owners shadow everything.
        self.occluder_owners.resize(self.occluders.len(), 0);
        let id = self.occluder_owners.last().map_or(0, |o| (o & OWNER_ID_MASK) + 1) & OWNER_ID_MASK;
        let flag = if occ.self_shadow { 0 } else { NO_SELF_SHADOW }
            | if occ.shadows_casters { 0 } else { NOT_ON_CASTERS };
        let owner = id | flag | shadow_length_bits(occ.shadow_length);
        append_occluder_segments(&mut self.occluders, &mut self.occluder_tints, transform, occ, sprite_size);
        self.occluder_owners.resize(self.occluders.len(), owner);
    }

    /// Every loop of a `tilemap_occluder` as **one** occluder: a single owner
    /// for all of them, so the parity that spares an occluder's inside
    /// counts the rock round a cave and the cave's own outline together - a
    /// lamp in the cave lights the rock face it sees, and the rock beyond it
    /// is in shadow. As separate owners the cave's outline would shadow the
    /// very rock it bounds.
    pub fn add_occluder_loops(&mut self, transform: &Transform, occ: &LightOccluder, loops: &[Vec<[f32; 2]>]) {
        let mut loops = loops.iter().filter(|l| l.len() >= 3);
        let Some(first) = loops.next() else { return };
        self.add_occluder(transform, &LightOccluder { polygon: first.clone(), traced: None, ..occ.clone() }, None);
        let owner = *self.occluder_owners.last().expect("add_occluder gives every edge an owner");
        for l in loops {
            let one = LightOccluder { polygon: l.clone(), traced: None, ..occ.clone() };
            append_occluder_segments(&mut self.occluders, &mut self.occluder_tints, transform, &one, None);
            self.occluder_owners.resize(self.occluders.len(), owner);
        }
    }

    /// [`Self::add_occluder`] for each loop of a `from_alpha` occluder's
    /// traced outline - each loop an owner of its own, so the parity that
    /// spares an occluder's inside counts one island at a time.
    pub fn add_traced_occluder(&mut self, transform: &Transform, occ: &LightOccluder, loops: &[Vec<[f32; 2]>]) {
        for l in loops {
            let one = LightOccluder { polygon: l.clone(), traced: None, ..occ.clone() };
            self.add_occluder(transform, &one, None);
        }
    }
}

/// Resolves the local-space outline points (CCW) of an occluder before
/// the entity `Transform` is applied. Polygon shapes are returned
/// verbatim; box / from-sprite shapes resolve to four corners. The
/// `sprite_size` is the entity's sprite bounds when `from_sprite` is
/// set (caller supplies it, or `None` to fall back to `size`).
fn occluder_local_outline(occ: &LightOccluder, sprite_size: Option<[f32; 2]>) -> Vec<glam::Vec2> {
    if occ.polygon.len() >= 3 {
        return occ.polygon.iter().map(|p| glam::Vec2::new(p[0], p[1])).collect();
    }
    let size = if occ.from_sprite || occ.from_alpha {
        sprite_size.unwrap_or(occ.size)
    } else {
        occ.size
    };
    let hx = size[0] * 0.5;
    let hy = size[1] * 0.5;
    vec![
        glam::Vec2::new(-hx, -hy),
        glam::Vec2::new(hx, -hy),
        glam::Vec2::new(hx, hy),
        glam::Vec2::new(-hx, hy),
    ]
}

/// Computes the world-space edge segments of an occluder placed at
/// `transform`, appending `[x0, y0, x1, y1]` per edge into `out` and a
/// matching `[r, g, b, opacity]` shadow tint per edge into `tints`. The
/// outline is closed (last point connects back to the first). Disabled
/// occluders should be filtered by the caller.
///
/// This writes no owner, so the edges shadow their own inside whatever
/// `self_shadow` says - [`LightData::add_occluder`] is the whole job.
pub fn append_occluder_segments(
    out: &mut Vec<[f32; 4]>,
    tints: &mut Vec<[f32; 4]>,
    transform: &Transform,
    occ: &LightOccluder,
    sprite_size: Option<[f32; 2]>,
) {
    let local = occluder_local_outline(occ, sprite_size);
    if local.len() < 2 {
        return;
    }
    let tint = [
        occ.shadow_tint[0],
        occ.shadow_tint[1],
        occ.shadow_tint[2],
        occ.shadow_opacity,
    ];
    let (sin, cos) = transform.rotation.dsin_cos();
    let to_world = |p: glam::Vec2| -> glam::Vec2 {
        let sx = p.x * transform.scale.x;
        let sy = p.y * transform.scale.y;
        glam::Vec2::new(sx * cos - sy * sin, sx * sin + sy * cos) + transform.position
    };
    let n = local.len();
    for i in 0..n {
        let a = to_world(local[i]);
        let b = to_world(local[(i + 1) % n]);
        out.push([a.x, a.y, b.x, b.y]);
        tints.push(tint);
    }
}

/// Box-occluder convenience used by tests: returns the four world-space
/// edge segments of a `size` box at `transform`.
pub fn occluder_segments(transform: &Transform, occ: &LightOccluder) -> Vec<[f32; 4]> {
    let mut out = Vec::new();
    let mut tints = Vec::new();
    append_occluder_segments(&mut out, &mut tints, transform, occ, None);
    out
}

/// Gathers light data from the ECS world.
pub fn gather_lights(world: &hecs::World) -> LightData {
    let mut data = LightData::default();

    if let Some((_entity, ambient)) = world.query::<&AmbientLight>().iter().next() {
        data.ambient = ambient.clone();
    }
    data.occlusion = world.query::<&AmbientOcclusion>().iter().next().map(|(_, ao)| *ao);
    data.toon = world.query::<&ToonLighting>().iter().next().map(|(_, t)| *t);
    data.gi = world.query::<&crate::ecs::components::GlobalIllumination>().iter().next().map(|(_, g)| *g);
    crate::feature!("fog", { data.fog = crate::ecs::fog::placed(world) });

    for (_entity, (transform, w, light)) in
        world.query::<(&Transform, Option<&WorldTransform>, &PointLight2D)>().iter()
    {
        let t = placed(transform, w);
        // The turn rides in the light for its cookie.
        data.point_lights.push((t.position, PointLight2D { turn: t.rotation, ..light.clone() }));
    }

    // A hot liquid's lights (`ecs::fluid::Fluid::lights`), as
    // `tick_fluids` left them: no shadows, scaled by the particle's heat.
    crate::feature!("fluid", {
        for (_entity, f) in world.query::<&crate::ecs::fluid::Fluid>().iter() {
            let Some(l) = &f.lights else { continue };
            for &(position, heat) in &f.lit {
                data.point_lights.push((
                    position,
                    PointLight2D {
                        color: l.color,
                        intensity: l.intensity * heat,
                        radius: l.radius,
                        casts_shadows: false,
                        ..PointLight2D::default()
                    },
                ));
            }
        }
    });

    // Particles that give off light (`ParticleEmitter::light`), as
    // `particles::tick_particles` left them: no shadows, every layer.
    for (_entity, em) in world.query::<&crate::particles::ParticleEmitter>().iter() {
        for l in &em.lit {
            data.point_lights.push((
                l.position,
                PointLight2D {
                    color: l.color,
                    intensity: l.intensity,
                    radius: l.radius,
                    falloff: l.falloff,
                    casts_shadows: false,
                    ..PointLight2D::default()
                },
            ));
        }
    }

    // Lightning (`ecs::lightning`): while a strike shows, lights along it -
    // no shadows - and a lift in the ambient light, both by its brightness.
    #[cfg(feature = "lightning")]
    for (_entity, l) in world.query::<&crate::ecs::lightning::Lightning>().iter() {
        let b = l.now();
        if b <= 0.0 {
            continue;
        }
        data.ambient.intensity += l.flash.max(0.0) * b;
        if l.light_intensity <= 0.0 || l.light_radius <= 0.0 {
            continue;
        }
        for p in l.light_points() {
            data.point_lights.push((
                p,
                PointLight2D {
                    color: l.light_color,
                    intensity: l.light_intensity * b,
                    radius: l.light_radius,
                    casts_shadows: false,
                    ..PointLight2D::default()
                },
            ));
        }
    }

    for (_entity, (transform, w, light)) in
        world.query::<(&Transform, Option<&WorldTransform>, &SpotLight2D)>().iter()
    {
        let t = placed(transform, w);
        data.spot_lights.push((t.position, t.rotation, light.clone()));
    }

    for (_entity, (transform, w, light)) in
        world.query::<(&Transform, Option<&WorldTransform>, &DirectionalLight2D)>().iter()
    {
        data.directional_lights.push((placed(transform, w).rotation, light.clone()));
    }

    for (_entity, (transform, w, occ, sprite)) in world
        .query::<(&Transform, Option<&WorldTransform>, &LightOccluder, Option<&SpriteComponent>)>()
        .iter()
    {
        let transform = &placed(transform, w);
        if !occ.enabled {
            continue;
        }
        // A traced silhouette, once the sprite's pixels have been read;
        // the sprite's box until then.
        if occ.from_alpha
            && let Some(t) = occ.traced.as_ref().filter(|t| !t.loops.is_empty())
            && occ.polygon.len() < 3
        {
            data.add_traced_occluder(transform, occ, &t.loops);
            continue;
        }
        let sprite_size = if occ.from_sprite || occ.from_alpha {
            sprite.and_then(|s| s.natural_size()).map(|v| [v.x, v.y])
        } else {
            None
        };
        data.add_occluder(transform, occ, sprite_size);
    }

    // Tilemaps' shadow layers: merged outlines, one owner per map.
    for (_entity, (transform, w, tm, occ)) in world
        .query::<(&Transform, Option<&WorldTransform>, &crate::tilemap::Tilemap, &TilemapOccluder)>()
        .iter()
    {
        if !occ.enabled || occ.layers.is_empty() {
            continue;
        }
        let loops = occ.cache.loops(tm, &occ.layers);
        data.add_occluder_loops(&placed(transform, w), &occ.as_light_occluder(), &loops);
    }

    for (_entity, (t, w, lm)) in
        world.query::<(&Transform, Option<&WorldTransform>, &LightmapComponent)>().iter()
    {
        if lm.texture_id.is_empty() {
            continue;
        }
        let t = &placed(t, w);
        data.lightmaps.push(LightmapQuad {
            center: t.position,
            rotation: t.rotation,
            size: glam::Vec2::new(lm.size[0] * t.scale.x, lm.size[1] * t.scale.y),
            uv_rect: lm.uv_rect,
            intensity: lm.intensity,
            texture_id: lm.texture_id.clone(),
        });
    }
    // Light flooded through tilemaps is drawn as lightmaps too.
    #[cfg(feature = "tile_light")]
    data.lightmaps.extend(crate::ecs::tile_light::quads(world));

    #[cfg(feature = "freeform_light")]
    for (_entity, (t, w, ff)) in world
        .query::<(&Transform, Option<&WorldTransform>, &crate::renderer::freeform_light::FreeformLight)>()
        .iter()
    {
        let t = &placed(t, w);
        if let Some(p) = crate::renderer::freeform_light::place(ff, t.position, t.rotation, t.scale) {
            data.freeform.push(p);
        }
    }

    data
}

/// Broad-phase shadow cull: returns `true` when any occluder segment lies
/// within `radius` of a light at `pos` — i.e. the light could possibly be
/// shadowed by something. A light's shadow rays all stay inside its disc
/// (radius `radius` around `pos`), so a segment whose closest point is
/// farther than `radius` can never block one and is irrelevant.
///
/// Callers set a per-light flag from this so lights with no occluder in
/// range skip the per-fragment occluder loop entirely on the GPU — a cheap
/// `O(lights × segments)` CPU pass that removes a potentially large
/// `O(fragments × segments)` GPU cost.
pub fn light_reaches_occluder(pos: glam::Vec2, radius: f32, occluders: &[[f32; 4]]) -> bool {
    let r2 = radius * radius;
    occluders.iter().any(|s| {
        let a = glam::Vec2::new(s[0], s[1]);
        let b = glam::Vec2::new(s[2], s[3]);
        let ab = b - a;
        let denom = ab.length_squared().max(1e-6);
        let t = ((pos - a).dot(ab) / denom).clamp(0.0, 1.0);
        let closest = a + ab * t;
        pos.distance_squared(closest) <= r2
    })
}

/// How many occluder edges the lighting pass holds per frame, **counted per
/// light**: each light is handed only the edges within its reach
/// ([`light_reach`]), nearest first, and an edge two lights can both see is
/// counted twice.
///
/// Until Sep 2026 the budget was sixty-four edges for the whole screen,
/// global and truncating: every light tested the same first sixty-four
/// segments it was handed, wherever they were, and the rest cast nothing -
/// sixteen boxes, and shadows that worked in one room and stopped in the
/// next. Per-light lists spend the pool only where a light can use it, and
/// the cull is exact: an edge outside a light's reach cannot cross any ray
/// that light casts, so shadows are what testing every edge would give until
/// the pool is full.
///
/// 448 fills the WGSL `Occluders` uniform to 14 KB (a count header plus a
/// segment and a tint per edge), inside the 16 KB a WebGL2 uniform buffer is
/// guaranteed. **One definition**, here rather than in the wgpu backend,
/// because the CPU light sampler must pack exactly as the GPU does - a
/// simulation that honoured shadows the screen does not draw would be worse
/// than one with none. `tests/cpu_light_matches_the_shader.rs` fails if a
/// shader's copy drifts.
pub const OCCLUDER_POOL: usize = 448;

/// Lights of each kind (point, spot) the lighting pass draws per frame;
/// lights past it are dropped. Packing honours it too, so a dropped light
/// spends none of [`OCCLUDER_POOL`].
///
/// A bound, not a budget: the lighting pass's buffer grows with the lights
/// a frame draws (256 bytes a light), so a scene pays for what it has. It
/// was a fixed allocation of 256 until Sep 2026, then 1024; what a light
/// really costs is its quad of screen, so a scene with thousands on screen
/// at once pays in fill rate long before it meets this.
pub const MAX_LIGHTS_PER_KIND: usize = 16384;

/// Whether a light gives no light at all: no intensity, or black. Such a
/// light is skipped - by the GPU pass, and by the packing and [`light_at`],
/// which agree because both read [`point_lights_and_suns`] (spot lights are
/// skipped by [`lights_for_view`], and add nothing to `light_at` anyway).
pub fn is_dark(intensity: f32, color: [f32; 3]) -> bool {
    intensity <= 0.0 || color.iter().all(|c| *c <= 0.0)
}

/// How far out along its direction a sun is placed ([`sun_as_point`]):
/// far enough that its rays across any view are parallel to a small fraction
/// of a degree (a 1280-unit view spans about two), near enough that `f32`
/// keeps a hundredth of a unit across it.
pub const SUN_DISTANCE: f32 = 40_000.0;

/// A sun ([`DirectionalLight2D`]) as the point light that draws it: placed
/// [`SUN_DISTANCE`] back along its direction from `anchor` (the view's
/// middle), reaching twice that, with no falloff - so across the view its
/// light is even and its rays parallel. Its softness, an angle, becomes the
/// emitter size that angle subtends from there. Also returns the light's
/// **shadow scale**: `1 / height`, which bounded shadows are stretched by -
/// a low sun, long shadows.
pub fn sun_as_point(
    rotation: f32,
    sun: &DirectionalLight2D,
    anchor: glam::Vec2,
) -> (glam::Vec2, PointLight2D, f32) {
    let angle = rotation + sun.direction;
    let travel = glam::Vec2::new(angle.dcos(), angle.dsin());
    let height = sun.height.max(0.02);
    let light = PointLight2D {
        color: sun.color,
        intensity: sun.intensity,
        radius: SUN_DISTANCE * 2.0,
        falloff: 0.0,
        shadow_softness: sun.softness.max(0.0) * SUN_DISTANCE,
        height,
        casts_shadows: sun.casts_shadows,
        light_mask: sun.light_mask,
        ..Default::default()
    };
    (anchor - travel * SUN_DISTANCE, light, 1.0 / height)
}

/// Where suns are placed from: the middle of the view the frame is drawn
/// through ([`crate::renderer::frame_view`]), else the world's origin - a
/// game asking [`light_at`] before any frame has been drawn.
pub fn sun_anchor() -> glam::Vec2 {
    crate::renderer::frame_view::frame_view().map_or(glam::Vec2::ZERO, |v| v.center)
}

/// Every light the point pipeline draws, in the order the occluder packing,
/// the GPU pass and [`light_at`] all use: suns first (so the per-kind limit
/// never drops one), then point lights - each with its shadow scale.
///
/// A light that gives no light ([`is_dark`]) is left out: it adds nothing,
/// and drawing it cost a full shadow test per pixel it covers. A town's
/// lamps by day and its moon by day are exactly that, and in the showcase's
/// Day & Night they were most of the frame.
pub fn point_lights_and_suns(lights: &LightData) -> Vec<(glam::Vec2, std::borrow::Cow<'_, PointLight2D>, f32)> {
    let anchor = sun_anchor();
    let mut out = Vec::with_capacity(lights.directional_lights.len() + lights.point_lights.len());
    for (rotation, sun) in &lights.directional_lights {
        if is_dark(sun.intensity, sun.color) {
            continue;
        }
        let (pos, light, scale) = sun_as_point(*rotation, sun, anchor);
        out.push((pos, std::borrow::Cow::Owned(light), scale));
    }
    for (pos, light) in &lights.point_lights {
        if is_dark(light.intensity, light.color) {
            continue;
        }
        out.push((*pos, std::borrow::Cow::Borrowed(light), 1.0));
    }
    out.truncate(MAX_LIGHTS_PER_KIND);
    out
}

/// How far from a light an occluder edge can still shadow it: its radius
/// (no lit fragment is further) plus its shadow softness (the soft path
/// casts rays to points that far from the centre).
pub fn light_reach(radius: f32, shadow_softness: f32) -> f32 {
    radius.max(0.0) + shadow_softness.max(0.0)
}

/// The lights the pass should draw for a view `[min, max]` (world space):
/// every light whose reach overlaps it, nearest the view's middle first when
/// there are more than [`MAX_LIGHTS_PER_KIND`] of a kind. Borrowed, with no
/// copy, when nothing needs leaving out.
///
/// The pass used to take the first 256 lights in ECS order, wherever they
/// were: in a world with more, lights on screen went dark while lights off
/// it kept their slots - and a light far off screen still cost a draw. A
/// lantern festival or a lit city is thousands of lights, of which the
/// screen sees a few hundred (the showcase's Ten Thousand Lights).
///
/// The CPU's `light_at` answers from all lights and does not know the view,
/// so the two can disagree only where more than [`MAX_LIGHTS_PER_KIND`] lights of one kind reach
/// the view at once.
pub fn lights_for_view(lights: &LightData, min: glam::Vec2, max: glam::Vec2) -> std::borrow::Cow<'_, LightData> {
    let reaches = |p: glam::Vec2, reach: f32| {
        p.x + reach >= min.x && p.x - reach <= max.x && p.y + reach >= min.y && p.y - reach <= max.y
    };
    let all_point = lights.point_lights.iter().all(|(p, l)| reaches(*p, light_reach(l.radius, l.shadow_softness)));
    // A dark spot light is left out below; a dark point light is left out
    // by `point_lights_and_suns`, which every reader goes through.
    let all_spot = lights
        .spot_lights
        .iter()
        .all(|(p, _, l)| !is_dark(l.intensity, l.color) && reaches(*p, light_reach(l.radius, l.shadow_softness)));
    if all_point
        && all_spot
        && lights.point_lights.len() <= MAX_LIGHTS_PER_KIND
        && lights.spot_lights.len() <= MAX_LIGHTS_PER_KIND
    {
        return std::borrow::Cow::Borrowed(lights);
    }
    let centre = (min + max) * 0.5;
    // Nearest first by how far the light's edge is from the middle; a stable
    // sort keeps equals in world order, so the chosen set does not shuffle.
    let edge = |p: glam::Vec2, reach: f32| (p.distance(centre) - reach).max(0.0);
    let mut point: Vec<_> = lights
        .point_lights
        .iter()
        .filter(|(p, l)| reaches(*p, light_reach(l.radius, l.shadow_softness)))
        .cloned()
        .collect();
    point.sort_by(|a, b| {
        edge(a.0, light_reach(a.1.radius, a.1.shadow_softness))
            .total_cmp(&edge(b.0, light_reach(b.1.radius, b.1.shadow_softness)))
    });
    point.truncate(MAX_LIGHTS_PER_KIND);
    let mut spot: Vec<_> = lights
        .spot_lights
        .iter()
        .filter(|(p, _, l)| !is_dark(l.intensity, l.color) && reaches(*p, light_reach(l.radius, l.shadow_softness)))
        .cloned()
        .collect();
    spot.sort_by(|a, b| {
        edge(a.0, light_reach(a.2.radius, a.2.shadow_softness))
            .total_cmp(&edge(b.0, light_reach(b.2.radius, b.2.shadow_softness)))
    });
    spot.truncate(MAX_LIGHTS_PER_KIND);
    std::borrow::Cow::Owned(LightData {
        point_lights: point,
        spot_lights: spot,
        directional_lights: lights.directional_lights.clone(),
        ambient: lights.ambient.clone(),
        occluders: lights.occluders.clone(),
        occluder_tints: lights.occluder_tints.clone(),
        occluder_owners: lights.occluder_owners.clone(),
        lightmaps: lights.lightmaps.clone(),
        occlusion: lights.occlusion,
        toon: lights.toon,
        freeform: lights.freeform.clone(),
        gi: lights.gi,
        fog: lights.fog,
    })
}

/// How much of the ambient light reaches `p` under ambient occlusion
/// ([`AmbientOcclusion`]): 1 far from every occluder, less near their
/// outlines. `segments` and `owners` are [`LightData::occluders`] and
/// [`LightData::occluder_owners`] - an occluder's edges are contiguous, as
/// [`LightData::add_occluder`] pushes them.
///
/// Each occluder counts once, by its nearest edge, and the counts multiply:
/// `1 - strength * (1 - d/radius)^2` per occluder within `radius`. A point
/// inside an occluder (an odd number of its edges crossed by a ray from it,
/// counted with [`ao_ray_crosses`]'s half-open rule) takes nothing from that
/// one. `ambient_occlusion.wgsl` transcribes this; `ambient_occlusion` in
/// the tests holds the two together.
pub fn ambient_occlusion_at(p: glam::Vec2, segments: &[[f32; 4]], owners: &[u32], ao: &AmbientOcclusion) -> f32 {
    let radius = ao.radius.max(1e-3);
    let strength = ao.strength.clamp(0.0, 1.0);
    let mut total = 1.0f32;
    let mut i = 0;
    while i < segments.len() {
        let owner = owners.get(i).copied().unwrap_or(0) & OWNER_ID_MASK;
        let mut nearest = f32::MAX;
        let mut crossings = 0u32;
        while i < segments.len() && (owners.get(i).copied().unwrap_or(0) & OWNER_ID_MASK) == owner {
            let s = &segments[i];
            nearest = nearest.min(segment_distance(p, s));
            if ao_ray_crosses(p, glam::Vec2::new(s[0], s[1]), glam::Vec2::new(s[2], s[3])) {
                crossings += 1;
            }
            i += 1;
        }
        if crossings % 2 == 0 && nearest < radius {
            let k = 1.0 - nearest / radius;
            total *= 1.0 - strength * k * k;
        }
    }
    total
}

/// Does the ray from `p` along [`AO_RAY`] cross the edge `a`-`b`? The inside
/// test of [`ambient_occlusion_at`]. An end on the ray's line counts as on its
/// left, so a ray through a corner crosses once (as [`ray_crosses`] does),
/// and any crossing ahead of `p` counts - [`ray_crosses`] ignores the first
/// thousandth of a ray to a light, which on a ray this long is every edge
/// near the point, and made every box darken its own inside.
pub fn ao_ray_crosses(p: glam::Vec2, a: glam::Vec2, b: glam::Vec2) -> bool {
    let d = AO_RAY;
    let side_a = d.x * (a.y - p.y) - d.y * (a.x - p.x) >= 0.0;
    let side_b = d.x * (b.y - p.y) - d.y * (b.x - p.x) >= 0.0;
    if side_a == side_b {
        return false;
    }
    let e = b - a;
    let denom = d.x * e.y - d.y * e.x;
    if denom.abs() < 1e-12 {
        return false;
    }
    let qp = a - p;
    (qp.x * e.y - qp.y * e.x) / denom > 0.0
}

/// The direction [`ao_ray_crosses`] counts along: off every axis, so it is
/// unlikely to run along an authored edge.
pub const AO_RAY: glam::Vec2 = glam::Vec2::new(1.0, 0.017);

/// The occluder edges ambient occlusion needs for a view `[min, max]`: every
/// occluder any of whose edges comes within `radius` of it, **whole** (the
/// inside test counts all of an occluder's edges), in world order, up to
/// [`OCCLUDER_POOL`] edges. Returns the edges, their owners, and how many
/// occluders did not fit.
pub fn ao_edges_for_view(
    lights: &LightData,
    min: glam::Vec2,
    max: glam::Vec2,
    radius: f32,
) -> (Vec<[f32; 4]>, Vec<u32>, usize) {
    let (lo, hi) = (min - glam::Vec2::splat(radius), max + glam::Vec2::splat(radius));
    let near = |s: &[f32; 4]| {
        s[0].max(s[2]) >= lo.x && s[0].min(s[2]) <= hi.x && s[1].max(s[3]) >= lo.y && s[1].min(s[3]) <= hi.y
    };
    let owner = |i: usize| lights.occluder_owners.get(i).copied().unwrap_or(0) & OWNER_ID_MASK;
    let (mut segs, mut owners, mut dropped) = (Vec::new(), Vec::new(), 0);
    let mut i = 0;
    while i < lights.occluders.len() {
        let start = i;
        let id = owner(i);
        while i < lights.occluders.len() && owner(i) == id {
            i += 1;
        }
        let group = &lights.occluders[start..i];
        if !group.iter().any(near) {
            continue;
        }
        if segs.len() + group.len() > OCCLUDER_POOL {
            dropped += 1;
            continue;
        }
        segs.extend_from_slice(group);
        owners.extend((start..i).map(|k| lights.occluder_owners.get(k).copied().unwrap_or(0)));
    }
    (segs, owners, dropped)
}

/// What `ambient_occlusion.wgsl` reads beside [`ao_edges_for_view`]'s edges,
/// in the two slots of the occluder pool it has no other use for: each
/// edge's **tint** holds its occluder's bounding box `[min_x, min_y, max_x,
/// max_y]`, and its **owner** word the index one past the occluder's last
/// edge. A pixel farther than the radius from the box can take nothing from
/// that occluder - its nearest edge is farther still - so the shader jumps
/// over all its edges after one test. Before, every pixel measured every
/// edge near the view (the showcase's Clay: 212 edges, ~6 ms a frame).
pub fn ao_occluder_bounds(segs: &[[f32; 4]], owners: &[u32]) -> (Vec<[f32; 4]>, Vec<u32>) {
    let owner = |i: usize| owners.get(i).copied().unwrap_or(0) & OWNER_ID_MASK;
    let (mut boxes, mut ends) = (Vec::with_capacity(segs.len()), Vec::with_capacity(segs.len()));
    let mut i = 0;
    while i < segs.len() {
        let start = i;
        let mut bb = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
        while i < segs.len() && owner(i) == owner(start) {
            let s = segs[i];
            bb = [bb[0].min(s[0]).min(s[2]), bb[1].min(s[1]).min(s[3]), bb[2].max(s[0]).max(s[2]), bb[3].max(s[1]).max(s[3])];
            i += 1;
        }
        boxes.extend(std::iter::repeat_n(bb, i - start));
        ends.extend(std::iter::repeat_n(i as u32, i - start));
    }
    (boxes, ends)
}

/// Shortest distance from `p` to the segment `[x0, y0, x1, y1]`.
pub fn segment_distance(p: glam::Vec2, s: &[f32; 4]) -> f32 {
    let a = glam::Vec2::new(s[0], s[1]);
    let b = glam::Vec2::new(s[2], s[3]);
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// The occluder edges each light tests, packed as the lighting pass uploads
/// them: one pool, and a `(start, count)` range into it per light.
#[derive(Debug, Clone, Default)]
pub struct PackedOccluders {
    /// `[x0, y0, x1, y1]` per entry, grouped by light.
    pub segments: Vec<[f32; 4]>,
    /// `[r, g, b, opacity]`, parallel to `segments`.
    pub tints: Vec<[f32; 4]>,
    /// Owners (`LightData::occluder_owners`), parallel to `segments`.
    pub owners: Vec<u32>,
    /// `(start, count)` into `segments`, one per point light in
    /// [`point_lights_and_suns`] order - suns first (up to
    /// [`MAX_LIGHTS_PER_KIND`] together).
    pub point: Vec<(usize, usize)>,
    /// The same for spot lights.
    pub spot: Vec<(usize, usize)>,
    /// Per point light, the [`owner_bit`]s of the occluders that do not
    /// shadow themselves and have the light inside them
    /// ([`light_inside_mask`]).
    pub point_inside: Vec<u32>,
    /// The same for spot lights.
    pub spot_inside: Vec<u32>,
    /// Edges some light could see that did not fit in the pool. Nonzero is
    /// the only case where shadows differ from testing every edge.
    pub dropped: usize,
}

impl PackedOccluders {
    /// A light's edges, tints and owners.
    pub fn slice(&self, range: (usize, usize), light_inside: u32) -> Occluders<'_> {
        let (start, count) = range;
        Occluders {
            segments: &self.segments[start..start + count],
            tints: &self.tints[start..start + count],
            owners: &self.owners[start..start + count],
            light_inside,
        }
    }
}

/// The occluder edges one light tests - what its range of the GPU pool
/// holds.
#[derive(Debug, Clone, Copy, Default)]
pub struct Occluders<'a> {
    pub segments: &'a [[f32; 4]],
    /// `[r, g, b, opacity]`, parallel to `segments`.
    pub tints: &'a [[f32; 4]],
    /// Parallel to `segments`; shorter (or empty) means those edges shadow
    /// everything.
    pub owners: &'a [u32],
    /// [`light_inside_mask`] for this light.
    pub light_inside: u32,
}

impl<'a> Occluders<'a> {
    /// Edges that all shadow everything, as every edge did before
    /// `self_shadow` existed.
    pub fn plain(segments: &'a [[f32; 4]], tints: &'a [[f32; 4]]) -> Self {
        Self { segments, tints, owners: &[], light_inside: 0 }
    }

    /// Every occluder in `lights`, for a light at `pos` - unpacked, so every
    /// edge is tested whatever its distance.
    pub fn all(lights: &'a LightData, pos: glam::Vec2) -> Self {
        Self {
            segments: &lights.occluders,
            tints: &lights.occluder_tints,
            owners: &lights.occluder_owners,
            light_inside: light_inside_mask(pos, &lights.occluders, &lights.occluder_owners),
        }
    }

    fn owner(&self, i: usize) -> u32 {
        self.owners.get(i).copied().unwrap_or(0)
    }
}

/// The one bit an occluder owner stands for in a shadow test's inside mask:
/// its index's low five bits. Two occluders 32 apart share a bit, which only
/// matters when a point is inside both - see [`shadow_factor`].
pub fn owner_bit(owner: u32) -> u32 {
    1u32 << (owner & 31)
}

/// Which occluders that do not shadow themselves have `pos` inside them, as
/// [`owner_bit`]s - an even-odd test along a ray to +x over **every** edge of
/// each (so the caller passes all of them, not one light's reachable few).
/// The shadow test needs it because the ray from a pixel to a light crosses
/// an outline an odd number of times whether the pixel is inside or the
/// light is, and those two want opposite answers.
pub fn light_inside_mask(pos: glam::Vec2, segments: &[[f32; 4]], owners: &[u32]) -> u32 {
    let mut mask = 0u32;
    for (s, &o) in segments.iter().zip(owners) {
        if o & NO_SELF_SHADOW == 0 {
            continue;
        }
        let (a, b) = (glam::Vec2::new(s[0], s[1]), glam::Vec2::new(s[2], s[3]));
        if (a.y > pos.y) != (b.y > pos.y) {
            let x = a.x + (pos.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if x > pos.x {
                mask ^= owner_bit(o);
            }
        }
    }
    mask
}

/// Pack every light's occluder list, as the lighting pass uploads it and
/// [`light_at`] reads it. Three cases, tried in order:
///
/// 1. **Per light.** Each light (point lights, then spot lights) is handed
///    the edges within its [`light_reach`], nearest first. The cheapest for
///    the GPU - a light tests only what can shadow it - and what a cave with a
///    few lights gets.
/// 2. **Shared.** If those lists together overflow [`OCCLUDER_POOL`] but the
///    distinct edges any light can reach fit, every light that reaches
///    anything is handed that one list. Many lights over little geometry - a
///    field of flashes along one wall - would pay for the wall once per light
///    in case 1; here they pay once. Still exact: an edge out of a light's
///    reach cannot block it, so testing it too changes nothing.
/// 3. **Cut.** Neither fits: per-light lists, each cut nearest first while
///    the pool lasts, so shadows go missing at the edge of a light's reach,
///    where it is dimmest, and `dropped` says how many edges went.
pub fn pack_light_occluders(lights: &LightData) -> PackedOccluders {
    // Each light's reachable edges, nearest first.
    let near = |pos: glam::Vec2, reach: f32| -> Vec<usize> {
        let mut v: Vec<(f32, usize)> = lights
            .occluders
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                let d = segment_distance(pos, s);
                (d <= reach).then_some((d, i))
            })
            .collect();
        v.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        v.into_iter().map(|(_, i)| i).collect()
    };
    // Suns, then point lights: `point_lights_and_suns`, the order everything
    // reading this packing uses.
    let points = point_lights_and_suns(lights);
    let point_lists: Vec<Vec<usize>> = points
        .iter()
        .map(|(pos, l, _)| if l.casts_shadows { near(*pos, light_reach(l.radius, l.shadow_softness)) } else { Vec::new() })
        .collect();
    // A light that casts no shadows is handed no edges: the GPU and
    // `light_at` both read this packing, so neither shadows it.
    let spot_lists: Vec<Vec<usize>> = lights
        .spot_lights
        .iter()
        .take(MAX_LIGHTS_PER_KIND)
        .map(|(pos, _, l)| if l.casts_shadows { near(*pos, light_reach(l.radius, l.shadow_softness)) } else { Vec::new() })
        .collect();

    let tint = |i: usize| lights.occluder_tints.get(i).copied().unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let owner = |i: usize| lights.occluder_owners.get(i).copied().unwrap_or(0);
    let mut out = PackedOccluders::default();
    // Which self-shadow-free occluders each light sits inside - over every
    // edge, since a polygon's far side may be out of the light's reach.
    out.point_inside = points
        .iter()
        .map(|(pos, _, _)| light_inside_mask(*pos, &lights.occluders, &lights.occluder_owners))
        .collect();
    out.spot_inside = lights
        .spot_lights
        .iter()
        .take(MAX_LIGHTS_PER_KIND)
        .map(|(pos, _, _)| light_inside_mask(*pos, &lights.occluders, &lights.occluder_owners))
        .collect();
    let total: usize = point_lists.iter().chain(&spot_lists).map(Vec::len).sum();

    if total > OCCLUDER_POOL {
        let mut distinct: Vec<usize> = point_lists.iter().chain(&spot_lists).flatten().copied().collect();
        distinct.sort_unstable();
        distinct.dedup();
        if distinct.len() <= OCCLUDER_POOL {
            // Case 2: one shared list.
            for &i in &distinct {
                out.segments.push(lights.occluders[i]);
                out.tints.push(tint(i));
                out.owners.push(owner(i));
            }
            let n = distinct.len();
            let range = |list: &Vec<usize>| if list.is_empty() { (0, 0) } else { (0, n) };
            out.point = point_lists.iter().map(range).collect();
            out.spot = spot_lists.iter().map(range).collect();
            return out;
        }
    }

    // Case 1, and case 3 when the pool runs out.
    let take = |list: &Vec<usize>, out: &mut PackedOccluders| -> (usize, usize) {
        let start = out.segments.len();
        let kept = list.len().min(OCCLUDER_POOL - start);
        out.dropped += list.len() - kept;
        for &i in &list[..kept] {
            out.segments.push(lights.occluders[i]);
            out.tints.push(tint(i));
            out.owners.push(owner(i));
        }
        (start, kept)
    };
    for list in &point_lists {
        let r = take(list, &mut out);
        out.point.push(r);
    }
    for list in &spot_lists {
        let r = take(list, &mut out);
        out.spot.push(r);
    }
    out
}

/// Returns true if the world contains any light components that require a lighting pass.
pub fn has_lights(world: &hecs::World) -> bool {
    world.query::<&PointLight2D>().iter().next().is_some()
        || world.query::<&SpotLight2D>().iter().next().is_some()
        || world.query::<&DirectionalLight2D>().iter().next().is_some()
        || world.query::<&AmbientLight>().iter().next().is_some()
        // Particles that give off light (`ParticleEmitter::light`).
        || world.query::<&crate::particles::ParticleEmitter>().iter().any(|(_, e)| e.light.is_some())
        || tile_lights(world)
        // Lightning that lights what it strikes (`ecs::lightning`).
        || lightning_lights(world)
        // A lightmap is light too: a scene lit only by baked maps needs the
        // pass as much as one with a point light.
        || world
            .query::<&crate::ecs::components::LightmapComponent>()
            .iter()
            .next()
            .is_some()
}

/// Whether some lightning lights what it strikes (`ecs::lightning`).
#[cfg(feature = "lightning")]
fn lightning_lights(world: &hecs::World) -> bool {
    world
        .query::<&crate::ecs::lightning::Lightning>()
        .iter()
        .any(|(_, l)| (l.light_intensity > 0.0 && l.light_radius > 0.0) || l.flash > 0.0)
}

#[cfg(not(feature = "lightning"))]
fn lightning_lights(_: &hecs::World) -> bool {
    false
}

/// Whether some tilemap floods light (`ecs::tile_light`).
#[cfg(feature = "tile_light")]
fn tile_lights(world: &hecs::World) -> bool {
    world.query::<&crate::ecs::tile_light::TileLight>().iter().next().is_some()
}

#[cfg(not(feature = "tile_light"))]
fn tile_lights(_: &hecs::World) -> bool {
    false
}

// ---------------------------------------------------------------------------
// Sampling the light on the CPU
// ---------------------------------------------------------------------------

/// The epsilons `segment_blocks` uses along the fragment→light ray, so a
/// segment touching either endpoint does not count as blocking.
///
/// **These are the shader's numbers.** `point_light.wgsl` and `spot_light.wgsl`
/// both use `t > 0.001 && t < 0.999`, and
/// `tests/cpu_light_matches_the_shader.rs` fails if the two drift apart.
pub const RAY_EPS_NEAR: f32 = 0.001;
pub const RAY_EPS_FAR: f32 = 0.999;

/// On a long ray the near epsilon is a distance instead: at most this many
/// world units, [`ray_eps_near`]. A thousandth of a sun's 40,000-unit ray is
/// forty units, so every edge within forty units of a point failed to shadow
/// it - and failed to count for the inside test. Rays up to 500 units long
/// (every lamp) keep the thousandth, so nothing that existed moved.
pub const RAY_EPS_NEAR_UNITS: f32 = 0.5;

/// The near epsilon for a ray of `length`: [`RAY_EPS_NEAR`], or
/// [`RAY_EPS_NEAR_UNITS`] of it when that is less.
pub fn ray_eps_near(length: f32) -> f32 {
    RAY_EPS_NEAR.min(RAY_EPS_NEAR_UNITS / length.max(1e-6))
}

/// How many samples across the emitter disc a soft shadow takes. The shader's
/// `SHADOW_SAMPLES`.
pub const SHADOW_SAMPLES: i32 = 8;

/// The half-width of a spot light's cone edge, in **cosine** rather than
/// angle: `spot_light.wgsl` fades the cone with
/// `smoothstep(cos_cone - 0.05, cos_cone + 0.05, cos_frag)`.
///
/// Being in cosine space is why the edge is not a constant number of degrees -
/// a narrow cone has a softer-looking edge than a wide one for the same
/// number. That is the shader's behaviour, so it is this module's too.
pub const SPOT_EDGE: f32 = 0.05;

/// Does the occluder segment `a`→`b` cross the open segment `p`→`light`?
///
/// A direct transcription of `segment_blocks` in `point_light.wgsl`. It is
/// here, in the same module as the data both sides read, because the game and
/// the renderer answering this question differently is the worst failure this
/// system can have: a player standing in what the screen shows as shadow while
/// the simulation calls them lit, or the reverse. Neither is visible as a bug -
/// it just feels unfair.
pub fn segment_blocks(p: glam::Vec2, light: glam::Vec2, a: glam::Vec2, b: glam::Vec2) -> bool {
    let r = light - p;
    let s = b - a;
    let denom = r.x * s.y - r.y * s.x;
    if denom.abs() < 1e-6 {
        return false; // parallel - treat as no intersection
    }
    let qp = a - p;
    let t = (qp.x * s.y - qp.y * s.x) / denom; // along p -> light
    let u = (qp.x * r.y - qp.y * r.x) / denom; // along a -> b
    t > ray_eps_near(r.length()) && t < RAY_EPS_FAR && (0.0..=1.0).contains(&u)
}

/// Light transmitted through occluder `i` when its edge blocks the ray:
/// `mix(white, tint.rgb, tint.a)`. The shader's `occluder_transmit`.
fn occluder_transmit(tints: &[[f32; 4]], i: usize) -> glam::Vec3 {
    let t = tints.get(i).copied().unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let tint = glam::Vec3::new(t[0], t[1], t[2]);
    glam::Vec3::ONE.lerp(tint, t[3])
}

/// Does the ray `p`→`light` cross the edge `a`→`b`, for **counting**? The
/// shader's `ray_crosses`, which decides parity in [`ray_transmit`].
///
/// Not [`segment_blocks`]: that counts an edge whose end the ray touches, so a
/// ray through a polygon's corner counts both edges meeting there - or, by a
/// rounding, one of them. Either is harmless for multiplying shadow in (the
/// same tint again) and fatal for parity, where a corner decides inside from
/// outside: a light puzzle's diagonal pane, which a lamp's ray crosses exactly
/// corner to corner, came out "inside" and stopped colouring the light. Here
/// an end on the ray's line counts as on its left, the usual half-open rule,
/// so a ray through a corner crosses once and one grazing a corner zero or
/// two times.
fn ray_crosses(p: glam::Vec2, light: glam::Vec2, a: glam::Vec2, b: glam::Vec2) -> bool {
    let r = light - p;
    let side = |q: glam::Vec2| r.x * (q.y - p.y) - r.y * (q.x - p.x) >= 0.0;
    if side(a) == side(b) {
        return false;
    }
    let s = b - a;
    let denom = r.x * s.y - r.y * s.x;
    if denom.abs() < 1e-12 {
        return false;
    }
    let qp = a - p;
    let t = (qp.x * s.y - qp.y * s.x) / denom;
    t > ray_eps_near(r.length()) && t < RAY_EPS_FAR
}

/// Where along the ray from `p` to `light` it crosses the edge `a`-`b`, as a
/// distance from `p`, or `None` if it does not ([`segment_blocks`]'s test).
fn crossing_distance(p: glam::Vec2, light: glam::Vec2, a: glam::Vec2, b: glam::Vec2) -> Option<f32> {
    if !segment_blocks(p, light, a, b) {
        return None;
    }
    let r = light - p;
    let s = b - a;
    let denom = r.x * s.y - r.y * s.x;
    let qp = a - p;
    let t = (qp.x * s.y - qp.y * s.x) / denom;
    Some(t * r.length())
}

/// How much of a bounded occluder's shadow falls on a point `near` units past
/// it: `1.0` within its [`LightOccluder::shadow_length`], fading to `0.0` over
/// the last quarter. The shader's `shadow_reach`.
fn shadow_reach(owner: u32, near: f32, scale: f32) -> f32 {
    let len = owner_shadow_length(owner) * scale;
    1.0 - smoothstep(len * 0.75, len, near)
}

/// Light that reaches `frag` along one ray from `light`: the product of what
/// every edge it crosses lets through - except the edges of an occluder that
/// does not shadow itself and has `frag` inside it, and faded by distance for
/// an occluder whose shadow has a length. The shader's `ray_transmit`.
///
/// Inside is decided by parity along this same ray (an odd number of that
/// occluder's edges crossed means `frag` and the light are on opposite sides
/// of its outline), corrected by whether the light is inside it
/// (`light_inside`). Every edge that ray can cross is within the light's
/// reach, so the light's packed list holds them all and the parity is exact.
///
/// A bounded shadow is measured from the **nearest** crossing of its
/// occluder - the face the point sees - for every edge of that occluder.
/// Measured per edge, a box's far face finished fading a box-width after its
/// near face, and every bounded shadow ended in a step.
///
/// Occluders are told apart by [`owner_bit`], five bits of their index, so two
/// that share those bits and are *both* crossed can confuse each other: two
/// self-shadow-free occluders that both contain `frag` cancel out and shadow
/// it, and two bounded ones are measured from the nearer. Rare, and stated.
/// Which self-shadow-free occluders `frag` is inside, as owner bits: parity
/// along the ray to the light's centre, corrected by whether the light is
/// inside (`light_inside`). A property of the point, not of a ray - so a soft
/// light counts it once, not once per sample (the shader's `pixel_inside`).
fn pixel_inside(frag: glam::Vec2, center: glam::Vec2, occ: &Occluders, flags: u32) -> u32 {
    if flags & 1 == 0 {
        return 0;
    }
    let mut parity = 0u32;
    for (i, s) in occ.segments.iter().enumerate() {
        let o = occ.owner(i);
        let (a, b) = (glam::Vec2::new(s[0], s[1]), glam::Vec2::new(s[2], s[3]));
        if o & NO_SELF_SHADOW != 0 && ray_crosses(frag, center, a, b) {
            parity ^= owner_bit(o);
        }
    }
    parity ^ occ.light_inside
}

fn ray_transmit(frag: glam::Vec2, light: glam::Vec2, occ: &Occluders, flags: u32, scale: f32, inside: u32) -> glam::Vec3 {
    let seg = |s: &[f32; 4]| (glam::Vec2::new(s[0], s[1]), glam::Vec2::new(s[2], s[3]));
    let mut near = [f32::MAX; 32];
    if flags & 2 != 0 {
        for (i, s) in occ.segments.iter().enumerate() {
            let o = occ.owner(i);
            let (a, b) = seg(s);
            if owner_shadow_length(o) > 0.0
                && let Some(d) = crossing_distance(frag, light, a, b)
            {
                let k = (o & 31) as usize;
                near[k] = near[k].min(d);
            }
        }
    }
    let mut transmit = glam::Vec3::ONE;
    for (i, s) in occ.segments.iter().enumerate() {
        let (a, b) = seg(s);
        if segment_blocks(frag, light, a, b) {
            let o = occ.owner(i);
            if o & NO_SELF_SHADOW != 0 && inside & owner_bit(o) != 0 {
                continue;
            }
            let reach = if owner_shadow_length(o) > 0.0 { shadow_reach(o, near[(o & 31) as usize], scale) } else { 1.0 };
            transmit *= glam::Vec3::ONE.lerp(occluder_transmit(occ.tints, i), reach);
        }
    }
    transmit
}

/// The shader's `occ.w`: bit 0 when a light's edges include a
/// self-shadow-free one (so the parity pass runs), bit 1 when they include a
/// bounded one (so the nearest-crossing pass runs).
pub fn occluder_flags(owners: &[u32]) -> u32 {
    let free = owners.iter().any(|o| o & NO_SELF_SHADOW != 0) as u32;
    let bounded = owners.iter().any(|&o| owner_shadow_length(o) > 0.0) as u32;
    free | bounded << 1
}

/// Per-channel shadow transmittance at `frag` for a light at `center`, in
/// `0..1`: one is fully lit, zero fully shadowed. The shader's
/// `shadow_factor`, including the soft path.
pub fn shadow_factor(frag: glam::Vec2, center: glam::Vec2, light_size: f32, occ: Occluders) -> glam::Vec3 {
    shadow_factor_scaled(frag, center, light_size, occ, 1.0)
}

/// [`shadow_factor`] for a light whose bounded shadows are stretched by
/// `scale` - a sun's `1 / height` ([`sun_as_point`]).
pub fn shadow_factor_scaled(
    frag: glam::Vec2,
    center: glam::Vec2,
    light_size: f32,
    occ: Occluders,
    scale: f32,
) -> glam::Vec3 {
    // `occ` is one light's list - `pack_light_occluders` decides it, as it
    // does for the GPU - so every edge here is tested, as the shader tests
    // every edge in the light's range.
    if occ.segments.is_empty() {
        return glam::Vec3::ONE;
    }
    let to_light = center - frag;
    let dist = to_light.length();
    if dist < 1e-4 {
        return glam::Vec3::ONE;
    }
    // The shader's `occ.w`: skip the first pass when nothing needs it.
    let flags = occluder_flags(occ.owners);
    let inside = pixel_inside(frag, center, &occ, flags);

    if light_size < 0.001 {
        return ray_transmit(frag, center, &occ, flags, scale, inside);
    }

    let perp = glam::Vec2::new(-to_light.y, to_light.x) / dist;
    let mut acc = glam::Vec3::ZERO;
    for s in 0..SHADOW_SAMPLES {
        let off = (s as f32 / (SHADOW_SAMPLES - 1) as f32 - 0.5) * 2.0;
        let sample_center = center + perp * (off * light_size);
        acc += ray_transmit(frag, sample_center, &occ, flags, scale, inside);
    }
    acc / SHADOW_SAMPLES as f32
}

/// How much light one point light puts on a world point, shadows included.
///
/// `pow(clamp(1 - dist/radius, 0, 1), falloff) * color * intensity * shadow`,
/// which is `fs_main` in `point_light.wgsl`.
pub fn point_light_at(p: glam::Vec2, pos: glam::Vec2, light: &PointLight2D, occ: Occluders) -> glam::Vec3 {
    point_light_at_scaled(p, pos, light, occ, 1.0)
}

/// [`point_light_at`] with the light's bounded shadows stretched by
/// `shadow_scale` - how a sun draws ([`sun_as_point`]).
pub fn point_light_at_scaled(
    p: glam::Vec2,
    pos: glam::Vec2,
    light: &PointLight2D,
    occ: Occluders,
    shadow_scale: f32,
) -> glam::Vec3 {
    if light.radius <= 0.0 {
        return glam::Vec3::ZERO;
    }
    let dist = squashed_distance(p - pos, light.squash);
    let attenuation = (1.0 - (dist / light.radius).clamp(0.0, 1.0)).dpowf(light.falloff);
    if attenuation <= 0.0 {
        return glam::Vec3::ZERO;
    }
    // `occluders` is this light's own list (`pack_light_occluders`). The
    // shader's `cast_shadows` flag is only "that list is not empty", so
    // always computing here gives the same answer.
    let shadow = shadow_factor_scaled(p, pos, light.shadow_softness, occ, shadow_scale);
    let color = glam::Vec3::new(light.color[0], light.color[1], light.color[2]);
    color * light.intensity * attenuation * shadow
}

/// How far `d` from a light is for its falloff, its pool squashed top to
/// bottom by `squash` (`PointLight2D::squash`) - `squashed_distance` in
/// `point_light.wgsl`, clamped the same.
pub fn squashed_distance(d: glam::Vec2, squash: f32) -> f32 {
    let squash = if squash.is_finite() && squash > 0.0 { squash.clamp(0.05, 1.0) } else { 1.0 };
    glam::Vec2::new(d.x, d.y / squash).length()
}

/// WGSL's `smoothstep`, which Rust has no equivalent of.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    if (edge1 - edge0).abs() < f32::EPSILON {
        return if x < edge0 { 0.0 } else { 1.0 };
    }
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// How much light one spot light puts on a world point, cone and shadows
/// included - `fs_main` in `spot_light.wgsl`.
///
/// The point light's attenuation, times a cone term. `entity_rotation` is the
/// transform's rotation and is **added** to the component's `direction`, which
/// is how the wgpu pass builds `spot.x`: a searchlight is turned by turning
/// the entity, so the two must be summed in the same order here or the cone on
/// the CPU points somewhere the cone on screen does not.
pub fn spot_light_at(
    p: glam::Vec2,
    pos: glam::Vec2,
    entity_rotation: f32,
    light: &SpotLight2D,
    occ: Occluders,
) -> glam::Vec3 {
    if light.radius <= 0.0 {
        return glam::Vec3::ZERO;
    }
    let delta = p - pos;
    let dist = delta.length();
    let attenuation = (1.0 - (squashed_distance(delta, light.squash) / light.radius).clamp(0.0, 1.0)).dpowf(light.falloff);
    if attenuation <= 0.0 {
        return glam::Vec3::ZERO;
    }
    let angle = entity_rotation + light.direction;
    let dir = glam::Vec2::new(angle.dcos(), angle.dsin());
    // At the emitter itself the direction is undefined, and the shader says so
    // twice - `select(normalize(delta), dir, ...)` and then `cone = 1.0`. Both
    // land on "fully lit", so one branch says it here.
    let cone = if dist < RAY_EPS_NEAR {
        1.0
    } else {
        let cos_cone = light.angle.dcos();
        smoothstep(cos_cone - SPOT_EDGE, cos_cone + SPOT_EDGE, (delta / dist).dot(dir))
    };
    if cone <= 0.0 {
        return glam::Vec3::ZERO;
    }
    let shadow = shadow_factor(p, pos, light.shadow_softness, occ);
    let color = glam::Vec3::new(light.color[0], light.color[1], light.color[2]);
    color * light.intensity * attenuation * cone * shadow
}

/// **How much light reaches a world point**, ambient included - the value the
/// composite multiplies the scene by at that pixel.
///
/// This is the question a game actually wants to ask and the renderer never
/// exposed: can a monster see the player, is this corner dark enough to hide
/// in, has this plant had any light. Before this, a game had to guess, and a
/// guess that disagrees with the screen is not a bug anyone can see - it just
/// feels unfair.
///
/// Point **and spot** lights, both with shadows. Spot lights were a deliberate
/// hole here until Sep 2026 - "nothing uses them, and a wrong answer is worse
/// than a missing one" - which held right up until a game wanted a searchlight
/// that sweeps a field and reveals what it crosses. A searchlight the player
/// can see and the simulation cannot read is two rules for one thing, which is
/// the failure this function exists to prevent.
///
/// **Lightmaps are not included** - they are pixels, and this has only the
/// world. A scene with a `LightmapComponent` asks [`light_at_textured`].
pub fn light_at(p: glam::Vec2, lights: &LightData) -> glam::Vec3 {
    light_at_packed(p, lights, &pack_light_occluders(lights))
}

/// [`light_at`] with the occluders already packed - for a caller asking about
/// many points in one frame, which should pack once
/// ([`pack_light_occluders`]) rather than once a point.
pub fn light_at_packed(p: glam::Vec2, lights: &LightData, packed: &PackedOccluders) -> glam::Vec3 {
    toon(light_summed(p, lights, packed, &|_, _| glam::Vec3::ONE, ALL_LIGHT_LAYERS), lights, p)
}

/// [`light_at`] for a sprite on the light layers `layers`
/// ([`crate::ecs::components::LightLayers`]): a light whose `light_mask`
/// shares no bit with them gives it nothing, as on the screen.
pub fn light_at_on(p: glam::Vec2, lights: &LightData, layers: u32) -> glam::Vec3 {
    toon(light_summed(p, lights, &pack_light_occluders(lights), &|_, _| glam::Vec3::ONE, layers), lights, p)
}

/// What a light's cookie lets through at `p` (`PointLight2D::cookie`): the
/// cookie's picture across `reach` each way from the light, turned by
/// `turn`, sampled by `sample(id, uv)` (uv 0..1, v down). No cookie is all of
/// it; outside the picture, none. `cookie.wgsl`'s `cookie_factor`, line for
/// line.
pub fn cookie_factor(
    p: glam::Vec2,
    center: glam::Vec2,
    turn: f32,
    reach: f32,
    cookie: &str,
    sample: &dyn Fn(&str, glam::Vec2) -> glam::Vec3,
) -> glam::Vec3 {
    if cookie.is_empty() {
        return glam::Vec3::ONE;
    }
    let d = p - center;
    let (s, c) = turn.dsin_cos();
    let local = glam::Vec2::new(d.x * c + d.y * s, -d.x * s + d.y * c) / reach.max(1e-4);
    if local.x.abs() > 1.0 || local.y.abs() > 1.0 {
        return glam::Vec3::ZERO;
    }
    sample(cookie, glam::Vec2::new(local.x * 0.5 + 0.5, 0.5 - local.y * 0.5))
}

/// A cookie's texel at `uv`, nearest, sRGB-decoded and times its alpha - as
/// the GPU samples it, near enough (it filters).
pub fn sample_cookie(img: &crate::asset::RawTextureData, uv: glam::Vec2) -> glam::Vec3 {
    if img.width == 0 || img.height == 0 {
        return glam::Vec3::ONE;
    }
    let tx = ((uv.x * img.width as f32).floor() as i64).clamp(0, img.width as i64 - 1) as usize;
    let ty = ((uv.y * img.height as f32).floor() as i64).clamp(0, img.height as i64 - 1) as usize;
    let i = (ty * img.width as usize + tx) * 4;
    let Some(px) = img.rgba.get(i..i + 4) else { return glam::Vec3::ONE };
    let a = px[3] as f32 / 255.0;
    glam::Vec3::new(srgb_to_linear(px[0]), srgb_to_linear(px[1]), srgb_to_linear(px[2])) * a
}

/// Toon lighting, when the frame has it, on light already summed - which is
/// first kept from going below black, as the composite keeps it: a
/// subtractive light can take more than is there.
fn toon(total: glam::Vec3, lights: &LightData, p: glam::Vec2) -> glam::Vec3 {
    let total = total.max(glam::Vec3::ZERO);
    match lights.toon {
        Some(toon) => toon_step(total, &toon, p),
        None => total,
    }
}

/// Every light at `p`, added up - what the composite steps into toon bands.
/// `cookie` samples a light's cookie (`sample_cookie`); [`light_at`] has no
/// textures and passes white, so it cannot see a cookie, as it cannot see a
/// lightmap.
fn light_summed(
    p: glam::Vec2,
    lights: &LightData,
    packed: &PackedOccluders,
    cookie: &dyn Fn(&str, glam::Vec2) -> glam::Vec3,
    layers: u32,
) -> glam::Vec3 {
    let amb = &lights.ambient;
    let ao = lights
        .occlusion
        .map_or(1.0, |ao| ambient_occlusion_at(p, &lights.occluders, &lights.occluder_owners, &ao));
    let mut total = glam::Vec3::new(amb.color[0], amb.color[1], amb.color[2]) * amb.intensity * ao;
    for (i, ((pos, light, scale), range)) in point_lights_and_suns(lights).iter().zip(&packed.point).enumerate() {
        if light.light_mask & layers == 0 {
            continue;
        }
        let occ = packed.slice(*range, packed.point_inside.get(i).copied().unwrap_or(0));
        let through = cookie_factor(p, *pos, light.turn, light.radius * light.cookie_scale, &light.cookie, cookie);
        // A subtractive light (`subtract`) takes its light away.
        let sign = if light.subtract { -1.0 } else { 1.0 };
        total += point_light_at_scaled(p, *pos, light, occ, *scale) * through * sign;
    }
    for (i, ((pos, rotation, light), range)) in lights.spot_lights.iter().zip(&packed.spot).enumerate() {
        if light.light_mask & layers == 0 {
            continue;
        }
        let occ = packed.slice(*range, packed.spot_inside.get(i).copied().unwrap_or(0));
        let through = cookie_factor(
            p,
            *pos,
            *rotation + light.direction,
            light.radius * light.cookie_scale,
            &light.cookie,
            cookie,
        );
        let sign = if light.subtract { -1.0 } else { 1.0 };
        total += spot_light_at(p, *pos, *rotation, light, occ) * through * sign;
    }
    #[cfg(feature = "freeform_light")]
    for ff in &lights.freeform {
        total += crate::renderer::freeform_light::light_at(p, ff);
    }
    total
}

/// The 4x4 ordered-dither (Bayer) matrix, row by row - `light_composite.wgsl`
/// has the same table.
const BAYER4: [f32; 16] = [0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0];

/// The dither threshold of the cell of `cell` world units holding `p`, in
/// `(0, 1)`: the Bayer matrix tiled over the world from its origin.
pub fn bayer_threshold(p: glam::Vec2, cell: f32) -> f32 {
    let ix = (p.x / cell).floor().rem_euclid(4.0) as usize;
    let iy = (p.y / cell).floor().rem_euclid(4.0) as usize;
    (BAYER4[(iy & 3) * 4 + (ix & 3)] + 0.5) / 16.0
}

/// Light stepped into [`ToonLighting::bands`] - the composite's `toon_step`.
/// By the brightest channel, so a light's colour is kept: `m` becomes the
/// nearest band, `round(m * bands) / bands`, the step between two bands
/// eased over `softness` of a band - or, with [`ToonLighting::dither`],
/// drawn as an ordered dither over that width, which is why it needs the
/// world point `p`.
pub fn toon_step(light: glam::Vec3, toon: &ToonLighting, p: glam::Vec2) -> glam::Vec3 {
    if toon.bands == 0 {
        return light;
    }
    let m = light.max_element();
    if m <= 1e-6 {
        return light;
    }
    let bands = toon.bands as f32;
    let scaled = m * bands;
    let f = scaled.floor();
    let frac = scaled - f;
    let soft = toon.softness.clamp(0.0, 1.0);
    let half = soft * 0.5;
    let up = if toon.dither > 0.0 {
        let threshold = 0.5 + (bayer_threshold(p, toon.dither) - 0.5) * soft;
        if frac >= threshold { 1.0 } else { 0.0 }
    } else if half <= 1e-6 {
        if frac >= 0.5 { 1.0 } else { 0.0 }
    } else {
        smoothstep(0.5 - half, 0.5 + half, frac)
    };
    light * ((f + up) / bands / m)
}

/// An sRGB-encoded byte as the linear value a `Rgba8UnormSrgb` texture
/// samples to - every sprite texture is uploaded that way.
fn srgb_to_linear(byte: u8) -> f32 {
    let c = byte as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).dpowf(2.4) }
}

/// The light the baked lightmaps add at `p`, given their pixels.
///
/// `render_lightmaps_wgpu` draws each lightmap as a quad into the light
/// buffer through the **additive** sprite pipeline: the fragment is
/// `texel * (intensity, intensity, intensity, 1)` and the blend is
/// `src.rgb * src.a + dst`. The texture is sampled `Nearest` from an sRGB
/// upload, so a texel adds `linear(rgb) * intensity * alpha`, and that is
/// what this adds - with the quad's rotation and the `uv_rect` applied as the
/// vertex positions apply them. `render_golden`'s
/// `the_cpu_reads_a_lightmap_as_the_gpu_draws_it` compares the two.
///
/// `texture` answers a texture id with its decoded pixels; a lightmap whose
/// pixels it cannot supply adds nothing, which is what the screen shows too
/// until the texture arrives.
pub fn lightmap_light_at<'a>(
    p: glam::Vec2,
    lights: &LightData,
    texture: impl Fn(&str) -> Option<&'a crate::asset::RawTextureData>,
) -> glam::Vec3 {
    let mut total = glam::Vec3::ZERO;
    for lm in &lights.lightmaps {
        if lm.size.x.abs() < 1e-6 || lm.size.y.abs() < 1e-6 {
            continue;
        }
        // Into the quad's own frame: unturn, then 0..1 across it, with v
        // running *down* from the top edge as the sprite batch lays it out.
        let d = p - lm.center;
        let (s, c) = lm.rotation.dsin_cos();
        let local = glam::Vec2::new(d.x * c + d.y * s, -d.x * s + d.y * c);
        let fx = local.x / lm.size.x + 0.5;
        let fy = 0.5 - local.y / lm.size.y;
        if !(0.0..1.0).contains(&fx) || !(0.0..1.0).contains(&fy) {
            continue;
        }
        let Some(img) = texture(&lm.texture_id) else { continue };
        if img.width == 0 || img.height == 0 {
            continue;
        }
        let [u0, v0, u1, v1] = lm.uv_rect;
        let u = u0 + (u1 - u0) * fx;
        let v = v0 + (v1 - v0) * fy;
        let tx = ((u * img.width as f32).floor() as i64).clamp(0, img.width as i64 - 1) as usize;
        let ty = ((v * img.height as f32).floor() as i64).clamp(0, img.height as i64 - 1) as usize;
        let i = (ty * img.width as usize + tx) * 4;
        let Some(px) = img.rgba.get(i..i + 4) else { continue };
        let a = px[3] as f32 / 255.0;
        let k = lm.intensity.max(0.0) * a;
        total += glam::Vec3::new(srgb_to_linear(px[0]), srgb_to_linear(px[1]), srgb_to_linear(px[2])) * k;
    }
    total
}

/// [`light_at`] with the baked lightmaps added - what the screen shows at `p`
/// when a scene has any.
///
/// `light_at` cannot see a lightmap, because a lightmap is pixels and
/// `LightData` is gathered from the world alone. Until Sep 2026 that was the
/// only question a game could ask, so a player standing in a baked sunbeam
/// was bright on screen and dark to the game - the disagreement this module
/// exists to prevent. Found by the arcade's light puzzle, whose sensors sit
/// under a stained-glass window.
pub fn light_at_textured<'a>(
    p: glam::Vec2,
    lights: &LightData,
    texture: impl Fn(&str) -> Option<&'a crate::asset::RawTextureData>,
) -> glam::Vec3 {
    // Toon lighting steps the whole sum, lightmaps included, as the composite
    // does - stepping the lights and then adding a lightmap would not.
    // Cookies (`sample_cookie`) and lightmaps both read the pictures.
    let cookie = |id: &str, uv: glam::Vec2| texture(id).map_or(glam::Vec3::ONE, |img| sample_cookie(img, uv));
    let summed = light_summed(p, lights, &pack_light_occluders(lights), &cookie, ALL_LIGHT_LAYERS) + lightmap_light_at(p, lights, &texture);
    toon(summed, lights, p)
}

/// Perceived brightness at a point, `0..1`-ish. Rec. 709 luma of [`light_at`],
/// which is what "is it dark here" means to a player.
pub fn brightness_at(p: glam::Vec2, lights: &LightData) -> f32 {
    let l = light_at(p, lights);
    0.2126 * l.x + 0.7152 * l.y + 0.0722 * l.z
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::PointLight2D;

    /// Mirror of the `segment_blocks` ray-vs-segment test in
    /// `point_light.wgsl`, kept in Rust so the shadow math is unit
    /// tested without a GPU.
    fn segment_blocks(p: glam::Vec2, light: glam::Vec2, a: glam::Vec2, b: glam::Vec2) -> bool {
        let r = light - p;
        let s = b - a;
        let denom = r.x * s.y - r.y * s.x;
        if denom.abs() < 1e-6 {
            return false;
        }
        let qp = a - p;
        let t = (qp.x * s.y - qp.y * s.x) / denom;
        let u = (qp.x * r.y - qp.y * r.x) / denom;
        t > 0.001 && t < 0.999 && (0.0..=1.0).contains(&u)
    }

    #[test]
    fn occluder_segments_axis_aligned() {
        let transform = Transform {
            position: glam::Vec2::new(10.0, 20.0),
            rotation: 0.0,
            scale: glam::Vec2::ONE,
        };
        let occ = LightOccluder {
            enabled: true,
            size: [4.0, 2.0],
            ..Default::default()
        };
        let segs = occluder_segments(&transform, &occ);
        // bottom-left = (8, 19), bottom-right = (12, 19)
        assert!((segs[0][0] - 8.0).abs() < 1e-5);
        assert!((segs[0][1] - 19.0).abs() < 1e-5);
        assert!((segs[0][2] - 12.0).abs() < 1e-5);
        assert!((segs[0][3] - 19.0).abs() < 1e-5);
        // Four edges form a closed loop: last segment ends at first start.
        assert!((segs[3][2] - segs[0][0]).abs() < 1e-5);
        assert!((segs[3][3] - segs[0][1]).abs() < 1e-5);
    }

    #[test]
    fn occluder_segments_respects_scale() {
        let transform = Transform {
            position: glam::Vec2::ZERO,
            rotation: 0.0,
            scale: glam::Vec2::new(2.0, 1.0),
        };
        let occ = LightOccluder {
            enabled: true,
            size: [2.0, 2.0],
            ..Default::default()
        };
        let segs = occluder_segments(&transform, &occ);
        // half-extent x = 2*0.5*2 = 2, so right edge at x = 2.
        assert!((segs[0][2] - 2.0).abs() < 1e-5);
    }

    #[test]
    fn gather_lights_collects_enabled_occluders_only() {
        let mut world = hecs::World::new();
        world.spawn((
            Transform::default(),
            PointLight2D {
                color: [1.0, 1.0, 1.0],
                intensity: 1.0,
                radius: 100.0,
                falloff: 1.0,
                shadow_softness: 0.0,
                height: crate::ecs::components::DEFAULT_LIGHT_HEIGHT,
                casts_shadows: true,
                ..Default::default()
            },
        ));
        world.spawn((
            Transform::default(),
            LightOccluder { enabled: true, size: [10.0, 10.0], ..Default::default() },
        ));
        world.spawn((
            Transform::default(),
            LightOccluder { enabled: false, size: [10.0, 10.0], ..Default::default() },
        ));
        let data = gather_lights(&world);
        // Only the enabled occluder contributes 4 segments.
        assert_eq!(data.occluders.len(), 4);
        assert_eq!(data.point_lights.len(), 1);
    }

    #[test]
    fn box_between_fragment_and_light_blocks_sight() {
        // Light at origin, fragment to the right past a box occluder
        // centered at (5, 0).
        let transform = Transform {
            position: glam::Vec2::new(5.0, 0.0),
            rotation: 0.0,
            scale: glam::Vec2::ONE,
        };
        let occ = LightOccluder { enabled: true, size: [2.0, 2.0], ..Default::default() };
        let segs = occluder_segments(&transform, &occ);
        let light = glam::Vec2::ZERO;
        let fragment = glam::Vec2::new(10.0, 0.0);
        let blocked = segs.iter().any(|s| {
            segment_blocks(
                fragment,
                light,
                glam::Vec2::new(s[0], s[1]),
                glam::Vec2::new(s[2], s[3]),
            )
        });
        assert!(blocked, "fragment behind the box should be in shadow");
    }

    #[test]
    fn fragment_with_clear_line_of_sight_is_lit() {
        // Box off to the side does not block a straight horizontal ray.
        let transform = Transform {
            position: glam::Vec2::new(5.0, 50.0),
            rotation: 0.0,
            scale: glam::Vec2::ONE,
        };
        let occ = LightOccluder { enabled: true, size: [2.0, 2.0], ..Default::default() };
        let segs = occluder_segments(&transform, &occ);
        let light = glam::Vec2::ZERO;
        let fragment = glam::Vec2::new(10.0, 0.0);
        let blocked = segs.iter().any(|s| {
            segment_blocks(
                fragment,
                light,
                glam::Vec2::new(s[0], s[1]),
                glam::Vec2::new(s[2], s[3]),
            )
        });
        assert!(!blocked, "fragment with clear sight should be lit");
    }

    #[test]
    fn polygon_occluder_emits_one_segment_per_edge() {
        // A triangle polygon yields three closed edges.
        let transform = Transform::default();
        let occ = LightOccluder {
            enabled: true,
            polygon: vec![[0.0, 0.0], [10.0, 0.0], [5.0, 10.0]],
            ..Default::default()
        };
        let segs = occluder_segments(&transform, &occ);
        assert_eq!(segs.len(), 3);
        // Last edge closes back to the first vertex (0, 0).
        assert!((segs[2][2] - 0.0).abs() < 1e-5);
        assert!((segs[2][3] - 0.0).abs() < 1e-5);
    }

    #[test]
    fn from_sprite_occluder_uses_sprite_bounds() {
        use crate::ecs::components::SpriteComponent;
        let mut world = hecs::World::new();
        world.spawn((
            Transform::default(),
            SpriteComponent {
                size: Some([8.0, 4.0]),
                ..Default::default()
            },
            LightOccluder {
                enabled: true,
                from_sprite: true,
                size: [999.0, 999.0], // ignored when from_sprite + sprite present
                ..Default::default()
            },
        ));
        let data = gather_lights(&world);
        assert_eq!(data.occluders.len(), 4);
        // Box half-extent x from sprite = 8*0.5 = 4: right edge at x = 4.
        let max_x = data
            .occluders
            .iter()
            .flat_map(|s| [s[0], s[2]])
            .fold(f32::MIN, f32::max);
        assert!((max_x - 4.0).abs() < 1e-5, "expected sprite-derived width, got {max_x}");
    }

    #[test]
    fn from_sprite_occluder_without_sprite_falls_back_to_size() {
        let mut world = hecs::World::new();
        world.spawn((
            Transform::default(),
            LightOccluder {
                enabled: true,
                from_sprite: true,
                size: [6.0, 6.0],
                ..Default::default()
            },
        ));
        let data = gather_lights(&world);
        assert_eq!(data.occluders.len(), 4);
        let max_x = data
            .occluders
            .iter()
            .flat_map(|s| [s[0], s[2]])
            .fold(f32::MIN, f32::max);
        assert!((max_x - 3.0).abs() < 1e-5);
    }

    #[test]
    fn gather_lights_emits_parallel_tints_per_segment() {
        let mut world = hecs::World::new();
        world.spawn((
            Transform::default(),
            LightOccluder {
                enabled: true,
                size: [10.0, 10.0],
                shadow_tint: [1.0, 0.0, 0.0],
                shadow_opacity: 0.5,
                ..Default::default()
            },
        ));
        let data = gather_lights(&world);
        // One tint entry per edge, all matching the occluder's settings.
        assert_eq!(data.occluder_tints.len(), data.occluders.len());
        assert_eq!(data.occluder_tints.len(), 4);
        for t in &data.occluder_tints {
            assert_eq!(*t, [1.0, 0.0, 0.0, 0.5]);
        }
    }

    #[test]
    fn default_occluder_tint_is_opaque_neutral() {
        // Default tint [0,0,0] + opacity 1 must reproduce the original
        // hard-shadow behaviour (mix(white, black, 1) == black == full
        // block in the shader).
        let mut world = hecs::World::new();
        world.spawn((
            Transform::default(),
            LightOccluder { enabled: true, size: [4.0, 4.0], ..Default::default() },
        ));
        let data = gather_lights(&world);
        assert_eq!(data.occluder_tints[0], [0.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn light_reaches_occluder_detects_in_range_segment() {
        // Box centred at (5, 0); a light at origin with radius 10 reaches it.
        let transform = Transform {
            position: glam::Vec2::new(5.0, 0.0),
            rotation: 0.0,
            scale: glam::Vec2::ONE,
        };
        let occ = LightOccluder { enabled: true, size: [2.0, 2.0], ..Default::default() };
        let segs = occluder_segments(&transform, &occ);
        assert!(light_reaches_occluder(glam::Vec2::ZERO, 10.0, &segs));
    }

    #[test]
    fn light_reaches_occluder_culls_out_of_range_segment() {
        // Same box, but a light with radius 2 cannot reach the box edge
        // (nearest edge is at x = 4, distance 4 > 2).
        let transform = Transform {
            position: glam::Vec2::new(5.0, 0.0),
            rotation: 0.0,
            scale: glam::Vec2::ONE,
        };
        let occ = LightOccluder { enabled: true, size: [2.0, 2.0], ..Default::default() };
        let segs = occluder_segments(&transform, &occ);
        assert!(!light_reaches_occluder(glam::Vec2::ZERO, 2.0, &segs));
        // No occluders at all ⇒ never reaches.
        assert!(!light_reaches_occluder(glam::Vec2::ZERO, 1000.0, &[]));
    }
}
