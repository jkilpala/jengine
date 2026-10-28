//! The ECS render systems: sprites (with materials, atlases and shadows),
//! lit sprites, shapes, text, tilemaps, particles, skinned meshes, entity UI
//! and `.gui` documents, all drawn through [`WgpuSpriteBatch`] /
//! [`WgpuShapeBatch`].
//!
//! Each system queues geometry and hands every texture- or pipeline-change to
//! a caller-supplied flush callback, so the host owns the render pass and the
//! texture lookup. That is also why the batch is single-texture: a flush per
//! run of same-texture draws.
//!
//! **The "gl path" these comments mention no longer exists.** wgpu replaced a
//! glow/OpenGL renderer (the Phase 8 glow cut), and these systems were first
//! written as its twins. Where a comment says "matches the gl path", it is
//! recording the behaviour this code was made to reproduce, not pointing at
//! code you can read.
//!
//! Enabled only with `--features gpu-wgpu`. `cargo check -p jengine-core`
//! without it skips this whole file.

#![cfg(feature = "gpu-wgpu")]

use glam::{Vec2, Vec4};
use hecs::World;

use crate::asset::sort_layers::SortLayers;
use crate::ecs::components::{GuiComponent, MaterialComponent, Mesh2D, ShadowCaster, SpriteComponent, SpriteFill, TextComponent, Transform, VectorPath, WorldTransform};
#[cfg(feature = "skinned_mesh")]
use crate::animation::skinned_mesh::{compute_skinned_positions, SkinnedMesh};
use crate::particles::{ParticleEmitter, ParticleSystem};
use crate::renderer::backend::wgpu::{
    WgpuLitSpritePipeline, WgpuShapeBatch, WgpuSpriteBatch,
};
use crate::renderer::camera::Camera2D;
use crate::renderer::frame_view::frame_view;
use crate::renderer::font_asset::FrameFonts;
use crate::renderer::text::FontData;
use crate::tilemap::Tilemap;
use crate::ui::gui_document::{GuiDocument, GuiNodeType, GuiTextAlign};
use crate::ui::layout::{compute_layout, LayoutRect};
use crate::ui::{TextAlign, UIButton, UIElement, UIImage, UIPanel, UIText};

/// Discriminator for [`render_ui_wgpu`]'s single flush callback.
///
/// Replaces the original two-callback design (`flush_quad` + `flush_font`)
/// because the wgpu runtime needs both branches to share `&mut AssetManager`
/// and `&mut wgpu::RenderPass<'_>`, and the borrow checker can't allow two
/// closures to coexist holding those captures.
pub enum UiFlush<'a> {
    /// Panel background quad. `None` means caller binds the 1x1 white
    /// fallback texture; `Some(id)` is an image resolved via `AssetManager`.
    Quad(Option<&'a str>),
    /// Bind a font atlas texture and flush the queued glyphs.
    ///
    /// Carries the texture id rather than implying the built-in atlas, so a
    /// widget whose `font_id` names a loaded `.font` draws from that font's
    /// atlas. Callers resolve it exactly like `Quad(Some(id))`.
    Font(&'a str),
    /// Restrict subsequent draws to a screen-space rect (`Some`) or lift the
    /// restriction (`None`). Emitted by [`render_gui_wgpu`] for nodes inside
    /// an `Overflow::Hidden` / `Overflow::Scroll` container; the caller maps
    /// it onto `RenderPass::set_scissor_rect`. Nothing is queued on the batch,
    /// so a caller handling this variant must return without flushing.
    ///
    /// **The rect is in framebuffer coordinates — origin top-left, y down —
    /// which is what `set_scissor_rect` wants, and is NOT the space the quads
    /// are in.** The quads are flipped into the caller's y-up GL ortho; the
    /// scissor is not, because the scissor test happens in framebuffer space.
    /// Passing the y-up value instead shifts the clip by
    /// `screen_h - 2y - height`, which reads as "the container clips a few
    /// dozen pixels too high" — content escapes past its top edge and the last
    /// row is cut off at the bottom. Already clamped to the surface.
    Clip(Option<LayoutRect>),
}

impl<'a> UiFlush<'a> {
    /// An image quad: the white texture when `id` is empty, which is what an
    /// image with no texture means (a speech bubble's plain card and tail).
    /// Passing `Some("")` on asked every host for a texture named "" - none
    /// has one, so the quad drew nothing (Iron Girders' HELP! bubble).
    pub fn image(id: &'a str) -> Self {
        if id.is_empty() { UiFlush::Quad(None) } else { UiFlush::Quad(Some(id)) }
    }
}

/// Queue every `Transform + SpriteComponent` entity onto `batch`.
///
/// The caller is responsible for `batch.begin()`, calling
/// `batch.flush(...)` with the bound texture, and supplying the
/// view-projection. Sprites are sorted by `z_order` so painter's-algorithm
/// layering matches the gl path.
///
/// A sprite with no explicit `size` draws at its texture's size, measured by
/// `resolve_sprite_regions`; `default_size` is only the fallback for one whose
/// texture is not known (not loaded yet, or the built-in white). Every runtime
/// passes [`SpriteComponent::FALLBACK_SIZE`].
/// The multi-texture sprite pass: one flush per texture run.
///
/// Sorts entities by `z_order`, then walks runs of identical
/// `texture_id`. For each run the batch is filled with that run's sprites
/// and `flush_group` is invoked with the texture id so the caller can look
/// up the matching [`super::super::renderer::backend::wgpu::WgpuTexture`]
/// and call `batch.flush(...)` against it. Returns the total number of
/// sprites queued (sum of sprites across all groups).
///
/// The caller is responsible for `begin_main_pass` / `present`. The system
/// handles `batch.begin()` between groups so callers only implement the
/// "look up texture and flush" half.
///
/// Sprites whose `texture_id` cannot be resolved (callback returns without
/// flushing, or the run is empty) are silently dropped — same behaviour as
/// the gl path when a texture is missing.
pub fn render_sprites_wgpu_multi(
    world: &World,
    sort_layers: &SortLayers,
    batch: &mut WgpuSpriteBatch,
    default_size: Vec2,
    mut flush_group: impl FnMut(&mut WgpuSpriteBatch, &str),
) -> usize {
    render_sprites_wgpu_multi_materialized(
        world,
        sort_layers,
        batch,
        default_size,
        |b, tex, _mat| flush_group(b, tex),
    )
}

/// Material-aware variant of [`render_sprites_wgpu_multi`].
///
/// Buckets sprites by `(material_id, texture_id)` so the caller can route
/// each run through a different render pipeline (e.g. a `.shader`
/// graph-built pipeline registered against the material). The third
/// callback argument is the entity's `MaterialComponent.material_id`,
/// or `None` if the entity has no `MaterialComponent` (default pipeline
/// expected).
///
/// Entities tagged `material_id == "lit_sprite"` are skipped — they route
/// through the dedicated lit-sprite pipeline (see [`render_lit_sprites_wgpu`]).
pub fn render_sprites_wgpu_multi_materialized(
    world: &World,
    sort_layers: &SortLayers,
    batch: &mut WgpuSpriteBatch,
    default_size: Vec2,
    flush_group: impl FnMut(&mut WgpuSpriteBatch, &str, Option<&str>),
) -> usize {
    sprites_materialized_filtered(world, sort_layers, batch, default_size, flush_group, None)
}

/// Every [`LightmapComponent`](crate::ecs::components::LightmapComponent) as
/// a quad centred on its entity, grouped by texture and flushed through
/// `flush(batch, texture_id)`. The caller flushes with
/// [`WgpuSpriteBatch::flush_additive`] into
/// [`WgpuLightingPass::light_buffer_pass`](crate::renderer::backend::wgpu::WgpuLightingPass::light_buffer_pass),
/// after `render_lights` and before the composite, so baked light adds to
/// the ambient and dynamic lights and is then multiplied into the scene.
///
/// `intensity` scales the texture's colour; alpha is kept, so a transparent
/// texel adds nothing.
///
/// **A lightmap can be drawn by a graph material** (Sep 2026): give its
/// entity a `material`, and `flush`'s third argument names it, with the
/// material's parameters already set on the batch - the host draws through
/// its graph pipeline instead of `flush_additive`. That is an animated,
/// procedural light: the showcase's Underwater lights its seabed with the
/// Voronoi caustics the Shader Gallery's water draws. Such a material should
/// blend `Additive`, as light adds. With a material the texture may be empty
/// (the white texture). `light_at_textured` cannot read a graph and skips
/// these, so gameplay asking the CPU about light does not see them.
pub fn render_lightmaps_wgpu(
    world: &World,
    batch: &mut WgpuSpriteBatch,
    mut flush: impl FnMut(&mut WgpuSpriteBatch, &str, Option<&str>),
) -> usize {
    type Map<'a> = (Transform, &'a crate::ecs::components::LightmapComponent, Option<&'a MaterialComponent>);
    let mut query = world.query::<(
        &Transform,
        Option<&WorldTransform>,
        &crate::ecs::components::LightmapComponent,
        Option<&MaterialComponent>,
    )>();
    // Placed by its world transform, as `gather_lights` places it for the
    // CPU, so a lightmap parented to something moves with it.
    let mut maps: Vec<Map<'_>> = query
        .iter()
        .filter(|(_, (_, _, lm, m))| !lm.texture_id.is_empty() || m.is_some())
        .map(|(_, (t, w, lm, m))| {
            let t = match w {
                Some(w) => Transform { position: w.position, rotation: w.rotation, scale: w.scale },
                None => t.clone(),
            };
            (t, lm, m)
        })
        .collect();
    let key = |m: &Map<'_>| {
        (
            crate::asset::resolve_texture_id(&m.1.texture_id).to_string(),
            m.2.map(|m| m.material_id.clone()),
            material_param_bits(m.2),
        )
    };
    maps.sort_by_key(|m| key(m));

    let mut total = 0;
    let mut i = 0;
    while i < maps.len() {
        let group = key(&maps[i]);
        batch.begin();
        while i < maps.len() && key(&maps[i]) == group {
            let (t, lm, _) = &maps[i];
            let k = lm.intensity.max(0.0);
            batch.draw_sprite_uv(
                t.position,
                Vec2::new(lm.size[0] * t.scale.x, lm.size[1] * t.scale.y),
                t.rotation,
                Vec4::new(k, k, k, 1.0),
                lm.uv_rect,
            );
            total += 1;
            i += 1;
        }
        batch.set_params(crate::renderer::material_params::from_bits(&group.2));
        flush(batch, &group.0, group.1.as_deref());
    }
    // Light flooded through tilemaps (`ecs::tile_light`): a quad of one
    // texel a cell over each map.
    #[cfg(feature = "tile_light")]
    for q in crate::ecs::tile_light::quads(world) {
        batch.begin();
        let k = q.intensity.max(0.0);
        batch.draw_sprite_uv(q.center, q.size, q.rotation, Vec4::new(k, k, k, 1.0), q.uv_rect);
        batch.set_params([[0.0; 4]; 4]);
        flush(batch, &q.texture_id, None);
        total += 1;
    }
    // Put back: the default shader reads `params[0]` as a text's edge.
    batch.set_params([[0.0; 4]; 4]);
    // Freeform lights (`renderer::freeform_light`): meshes added into the
    // same buffer, coloured by their falloff, against the white texture -
    // here so every host that draws lightmaps draws them too.
    #[cfg(feature = "freeform_light")]
    {
    let mut q = world.query::<(&Transform, Option<&WorldTransform>, &crate::renderer::freeform_light::FreeformLight)>();
    let placed: Vec<_> = q
        .iter()
        .filter_map(|(_, (t, w, ff))| {
            let (p, r, s) = w.map_or((t.position, t.rotation, t.scale), |w| (w.position, w.rotation, w.scale));
            crate::renderer::freeform_light::place(ff, p, r, s)
        })
        .collect();
    drop(q);
    if !placed.is_empty() {
        batch.begin();
        for light in &placed {
            let (pos, weights, indices) = crate::renderer::freeform_light::mesh(light);
            let uvq = vec![[0.5, 0.5, 1.0]; pos.len()];
            let colors: Vec<[f32; 4]> = weights.iter().map(|w| [light.color.x * w, light.color.y * w, light.color.z * w, 1.0]).collect();
            batch.draw_mesh(&pos, &uvq, &colors, &indices);
            total += 1;
        }
        flush(batch, crate::asset::WHITE_TEXTURE_ID, None);
    }
    }
    total
}

/// `only_unlit`: `None` draws every sprite (the unlit preview passes),
/// `Some(true)` only `Unlit` ones.
fn sprites_materialized_filtered(
    world: &World,
    sort_layers: &SortLayers,
    batch: &mut WgpuSpriteBatch,
    default_size: Vec2,
    mut flush_group: impl FnMut(&mut WgpuSpriteBatch, &str, Option<&str>),
    only_unlit: Option<bool>,
) -> usize {
    // Borrowed, not cloned. This used to clone a whole `SpriteComponent` per
    // sprite per frame — it owns a `String` texture id and an optional
    // `String` sort layer — then allocate another `String` for the resolved
    // texture id, then do a random `MaterialComponent` lookup per entity and
    // clone that `String` too. Three heap allocations per sprite per frame,
    // every frame, for data the renderer only reads.
    //
    // The material comes from the same query rather than a per-entity `get`,
    // and the sort key is resolved once here instead of twice per comparison
    // inside the sort.
    let mut query = world.query::<(
        &Transform,
        &SpriteComponent,
        Option<&WorldTransform>,
        Option<&MaterialComponent>,
        Option<&crate::ecs::components::Unlit>,
    )>();
    // What the pass can see, if the host said. Sprites outside it are
    // dropped before the sort, which is where their cost was.
    let view = frame_view();
    let ceiling = backdrop_ceiling(world, sort_layers);
    /// A sprite or a mesh, with its sort keys resolved once.
    struct Item<'a> {
        transform: WorldTransform,
        mat: Option<&'a str>,
        blend: crate::renderer::material::BlendMode,
        z: i32,
        y_key: i32,
        tex: &'a str,
        what: Drawable<'a>,
    }
    enum Drawable<'a> {
        Sprite(&'a SpriteComponent),
        Mesh(&'a Mesh2D),
        /// A `VectorPath`, tessellated for this frame.
        Path(Vec<crate::renderer::vector_path::PathPart>),
        /// A `SkinnedMesh`, skinned for this frame: world-space triangles.
        #[cfg(feature = "skinned_mesh")]
        Skinned(SkinnedTriangles),
    }
    let mut drawables: Vec<Item> = query
        .iter()
        .filter_map(|(_, (t, s, w, m, unlit))| {
            let transform = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
            if let Some(v) = view
                && !v.sees_sprite(transform.position, transform.scale, s, default_size)
            {
                return None;
            }
            let mat_id = m.map(|m| m.material_id.as_str());
            if only_unlit.is_some_and(|want| unlit.is_some() != want) {
                return None;
            }
            // Fully transparent through the default pipeline draws nothing:
            // skip it rather than batch it. Pools of reused entities (bullets,
            // particles) hide their free slots this way. A material's shader
            // may not read the vertex alpha, so those are kept.
            if mat_id.is_none() && s.color[3] <= 0.0 {
                return None;
            }
            let z = sort_layers.resolve(s.sort_layer.as_deref(), s.z_order);
            // Drawn already, beneath the tilemaps.
            if is_backdrop(z, mat_id, unlit.is_some(), ceiling) {
                return None;
            }
            Some(Item {
                y_key: s.y_sort_key(transform.position.y),
                tex: crate::asset::resolve_texture_id(&s.texture_id),
                transform,
                mat: mat_id,
                blend: sprite_blend(m),
                z,
                what: Drawable::Sprite(s),
            })
        })
        .collect();
    // Meshes join the same list under the same keys. Never a backdrop
    // (the tilemap pass draws sprites only), never y-sorted.
    let mut mesh_query = world.query::<(
        &Transform,
        &Mesh2D,
        Option<&WorldTransform>,
        Option<&MaterialComponent>,
        Option<&crate::ecs::components::Unlit>,
    )>();
    for (_, (t, mesh, w, m, unlit)) in mesh_query.iter() {
        let mat_id = m.map(|m| m.material_id.as_str());
        if only_unlit.is_some_and(|want| unlit.is_some() != want)
            || (mat_id.is_none() && mesh.color[3] <= 0.0)
            || mesh.vertices.len() < 3
        {
            continue;
        }
        let transform = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        if !view_sees_mesh(view.as_ref(), &transform, mesh) {
            continue;
        }
        drawables.push(Item {
            y_key: 0,
            tex: crate::asset::resolve_texture_id(&mesh.texture_id),
            transform,
            mat: mat_id,
            blend: sprite_blend(m),
            z: sort_layers.resolve(mesh.sort_layer.as_deref(), mesh.z_order),
            what: Drawable::Mesh(mesh),
        });
    }
    // Vector paths, under the same keys against the white texture - see
    // `renderer::vector_path`. They used to be a pass of their own after
    // every sprite, so no `z_order` could put one under a sprite.
    let mut path_query = world.query::<(
        &Transform,
        &VectorPath,
        Option<&WorldTransform>,
        Option<&crate::ecs::components::Unlit>,
    )>();
    for (_, (t, vp, w, unlit)) in path_query.iter() {
        if only_unlit.is_some_and(|want| unlit.is_some() != want) {
            continue;
        }
        let parts = crate::renderer::vector_path::tessellate(vp);
        if parts.is_empty() {
            continue;
        }
        let transform = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        if !view_sees_path(view.as_ref(), &transform, &parts) {
            continue;
        }
        drawables.push(Item {
            y_key: 0,
            tex: crate::asset::WHITE_TEXTURE_ID,
            transform,
            mat: None,
            blend: crate::renderer::material::BlendMode::Alpha,
            z: sort_layers.resolve(vp.sort_layer.as_deref(), vp.z_order),
            what: Drawable::Path(parts),
        });
    }
    // Skinned meshes too - skinned on the CPU against their bones, then
    // sorted like the sprite of their texture they are drawn as.
    #[cfg(feature = "skinned_mesh")]
    let mut skinned_query = world.query::<(
        &SkinnedMesh,
        &crate::ecs::components::SkeletonRoot,
        Option<&crate::ecs::components::Unlit>,
    )>();
    #[cfg(feature = "skinned_mesh")]
    for (root, (mesh, _, unlit)) in skinned_query.iter() {
        if only_unlit.is_some_and(|want| unlit.is_some() != want) {
            continue;
        }
        let Some(tris) = skin(world, root, mesh) else { continue };
        drawables.push(Item {
            y_key: 0,
            tex: crate::asset::resolve_texture_id(&mesh.texture_id),
            transform: WorldTransform::default(),
            mat: None,
            blend: crate::renderer::material::BlendMode::Alpha,
            z: sort_layers.resolve(None, mesh.z_order),
            what: Drawable::Skinned(tris),
        });
    }
    // Sort by resolved layer (back-to-front), then down the screen for
    // whoever asked for it, then by (material, texture) so adjacent
    // same-bucket sprites flush together.
    //
    // Position has to outrank the batching keys or it would not decide
    // anything: two sprites of the same texture are exactly the pair a
    // y-sorted band needs put in order. Sprites that did not opt in all
    // answer 0 here, so an ordinary band is grouped by texture as before
    // and pays nothing for this.
    drawables.sort_by(|a, b| {
        a.z.cmp(&b.z)
            .then_with(|| a.y_key.cmp(&b.y_key))
            .then_with(|| a.mat.cmp(&b.mat))
            .then_with(|| blend_rank(a.blend).cmp(&blend_rank(b.blend)))
            .then_with(|| a.tex.cmp(b.tex))
    });

    let mut total = 0usize;
    let mut i = 0;
    while i < drawables.len() {
        let group_mat = drawables[i].mat;
        // An empty texture id means "just tint the white pixel". Left as `""`
        // it reaches the flush callback, finds no registered texture, and the
        // whole batch is discarded — a sprite that vanishes with only a
        // console warning to say so.
        let group_tex = drawables[i].tex;
        let group_blend = drawables[i].blend;
        batch.begin();
        // `Unlit` sprites used to ignore this: they were drawn after the
        // lighting composite through a plain alpha flush, so a glow could
        // be additive (and darkened by the lights) or unlit (and
        // alpha-blended), never both.
        batch.set_blend(group_blend);
        let mut count = 0usize;
        while i < drawables.len()
            && drawables[i].mat == group_mat
            && drawables[i].tex == group_tex
            && drawables[i].blend == group_blend
        {
            // A run longer than the batch holds is flushed in pieces. Without
            // this the batch refused every quad past its capacity in
            // silence, so 16,000 boids on one texture drew as 8,192.
            if batch.is_full() {
                flush_group(batch, group_tex, group_mat);
                total += count;
                count = 0;
                batch.begin();
            }
            match &drawables[i].what {
                Drawable::Sprite(s) => queue_sprite(batch, &drawables[i].transform, s, default_size),
                Drawable::Mesh(mesh) => queue_mesh(batch, &drawables[i].transform, mesh),
                Drawable::Path(parts) => queue_path(batch, &drawables[i].transform, parts),
                #[cfg(feature = "skinned_mesh")]
                Drawable::Skinned(tris) => queue_skinned(batch, tris),
            }
            // A mesh bigger than a whole batch goes out in pieces.
            while batch.has_overflow() {
                let rest = batch.take_overflow();
                flush_group(batch, group_tex, group_mat);
                total += count;
                count = 0;
                batch.begin();
                batch.resume_overflow(rest);
            }
            count += 1;
            i += 1;
        }
        flush_group(batch, group_tex, group_mat);
        total += count;
    }
    batch.set_blend(crate::renderer::material::BlendMode::Alpha);
    total
}

#[inline]
fn queue_sprite(
    batch: &mut WgpuSpriteBatch,
    transform: &WorldTransform,
    sprite: &SpriteComponent,
    default_size: Vec2,
) {
    queue_sprite_tinted(batch, transform, sprite, default_size, Vec4::ONE);
}

/// [`queue_sprite`] with its colour multiplied by `mul` - `CUTOUT` draws the
/// sprite's shape in black at its own alpha (see [`WorldLayer`]).
#[inline]
fn queue_sprite_tinted(
    batch: &mut WgpuSpriteBatch,
    transform: &WorldTransform,
    sprite: &SpriteComponent,
    default_size: Vec2,
    mul: Vec4,
) {
    let color = Vec4::from(sprite.color) * mul;
    // A radial fill (`ecs::sprite_fill`): a fan from the middle, carried by
    // the world transform like the quad it replaces.
    if let Some(SpriteFill::Radial { amount, start, clockwise }) = sprite.fill.as_ref() {
        let uv = sprite.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let size = sprite.drawn_size(default_size);
        let (points, uvq, indices) = crate::ecs::sprite_fill::radial(size, sprite.anchor, uv, *amount, *start, *clockwise);
        if !indices.is_empty() {
            let positions: Vec<Vec2> = points.iter().map(|p| transform.apply(*p)).collect();
            let colors = vec![<[f32; 4]>::from(color); positions.len()];
            batch.draw_mesh(&positions, &uvq, &colors, &indices);
        }
        return;
    }
    // Explicit-corner quads (faked perspective walls) bypass the
    // size/anchor/fill/rotation path entirely — the script supplies the
    // exact geometry as local offsets from the entity position.
    if let Some(corners) = sprite.corners {
        let uv = sprite.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        batch.draw_sprite_quad(transform.position, corners, color, uv);
        return;
    }
    // A sheared world transform (a turned child under a squashed parent)
    // shears every quad about the entity's position, which is where the
    // shear sits in the map: after the scale, before the rotation.
    for_each_fill_quad(sprite, default_size, transform.scale, |sub_size, uv, sub_anchor| {
        batch.draw_sprite_anchored_skewed(
            transform.position,
            sub_size,
            transform.rotation,
            transform.shear,
            color,
            uv,
            sub_anchor,
        );
    });
}

/// Queue a [`Mesh2D`]: its vertices carried by the world transform, its
/// `uv` mapped into the resolved `uv_rect`, each colour multiplied by the
/// component's, and `q` folded into the projective texture coordinate.
fn queue_mesh(batch: &mut WgpuSpriteBatch, transform: &WorldTransform, mesh: &Mesh2D) {
    queue_mesh_tinted(batch, transform, mesh, Vec4::ONE);
}

fn queue_mesh_tinted(batch: &mut WgpuSpriteBatch, transform: &WorldTransform, mesh: &Mesh2D, mul: Vec4) {
    let tint = Vec4::from(mesh.color) * mul;
    let [u0, v0, u1, v1] = mesh.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
    let n = mesh.vertices.len();
    let mut positions: Vec<Vec2> = Vec::with_capacity(n);
    let mut uvq: Vec<[f32; 3]> = Vec::with_capacity(n);
    let mut colors: Vec<[f32; 4]> = Vec::with_capacity(n);
    for v in &mesh.vertices {
        positions.push(transform.apply(Vec2::from(v.position)));
        let q = if v.q.is_finite() && v.q > 0.0 { v.q } else { 1.0 };
        let u = u0 + (u1 - u0) * v.uv[0];
        let vv = v0 + (v1 - v0) * v.uv[1];
        uvq.push([u * q, vv * q, q]);
        colors.push((Vec4::from(v.color) * tint).into());
    }
    batch.draw_mesh(&positions, &uvq, &colors, &mesh.triangles());
}

/// Queue a tessellated [`VectorPath`]: each part's triangles carried by the
/// world transform, drawn against the white texture in the part's colour.
fn queue_path(
    batch: &mut WgpuSpriteBatch,
    transform: &WorldTransform,
    parts: &[crate::renderer::vector_path::PathPart],
) {
    queue_path_tinted(batch, transform, parts, Vec4::ONE);
}

fn queue_path_tinted(
    batch: &mut WgpuSpriteBatch,
    transform: &WorldTransform,
    parts: &[crate::renderer::vector_path::PathPart],
    mul: Vec4,
) {
    for part in parts {
        let positions: Vec<Vec2> = part.vertices.iter().map(|v| transform.apply(Vec2::from(*v))).collect();
        // Any texel of the white texture is white; the middle one is safest
        // under filtering.
        let uvq = vec![[0.5, 0.5, 1.0]; positions.len()];
        let colors = vec![(Vec4::from(part.color) * mul).into(); positions.len()];
        batch.draw_mesh(&positions, &uvq, &colors, &part.indices);
    }
}

/// A skinned mesh, skinned for one frame: world-space positions, and what
/// the batch needs to draw them.
#[derive(Default)]
pub(crate) struct SkinnedTriangles {
    positions: Vec<Vec2>,
    uvq: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

/// Skin one mesh against its bones for this frame. `None` when it has
/// nothing to draw.
#[cfg(feature = "skinned_mesh")]
fn skin(world: &World, root: hecs::Entity, mesh: &SkinnedMesh) -> Option<SkinnedTriangles> {
    if mesh.indices.len() < 3 || mesh.color[3] <= 0.0 {
        return None;
    }
    let positions = compute_skinned_positions(mesh, world, root);
    let n = positions.len();
    let indices: Vec<u32> = mesh
        .indices
        .chunks_exact(3)
        .filter(|t| t.iter().all(|&i| (i as usize) < n))
        .flatten()
        .copied()
        .collect();
    if indices.is_empty() {
        return None;
    }
    Some(SkinnedTriangles {
        uvq: mesh.vertices.iter().map(|v| [v.uv[0], v.uv[1], 1.0]).collect(),
        colors: vec![mesh.color; n],
        positions,
        indices,
    })
}

#[cfg(feature = "skinned_mesh")]
/// Queue a skinned mesh's triangles - already in world space.
fn queue_skinned(batch: &mut WgpuSpriteBatch, tris: &SkinnedTriangles) {
    queue_skinned_tinted(batch, tris, Vec4::ONE);
}

fn queue_skinned_tinted(batch: &mut WgpuSpriteBatch, tris: &SkinnedTriangles, mul: Vec4) {
    if mul == Vec4::ONE {
        batch.draw_mesh(&tris.positions, &tris.uvq, &tris.colors, &tris.indices);
        return;
    }
    let colors: Vec<[f32; 4]> = tris.colors.iter().map(|c| (Vec4::from(*c) * mul).into()).collect();
    batch.draw_mesh(&tris.positions, &tris.uvq, &colors, &tris.indices);
}

/// Whether the frame can see a path: a disc about its entity covering its
/// farthest vertex.
fn view_sees_path(
    view: Option<&crate::renderer::frame_view::FrameView>,
    transform: &WorldTransform,
    parts: &[crate::renderer::vector_path::PathPart],
) -> bool {
    match view {
        Some(v) => {
            let radius = crate::renderer::vector_path::bounding_radius(parts)
                * transform.scale.x.abs().max(transform.scale.y.abs());
            v.sees_disc(transform.position, radius)
        }
        None => true,
    }
}

/// Whether the frame can see a mesh: a disc about its entity covering its
/// farthest vertex.
fn view_sees_mesh(view: Option<&crate::renderer::frame_view::FrameView>, transform: &WorldTransform, mesh: &Mesh2D) -> bool {
    match view {
        Some(v) => {
            let radius = mesh.bounding_radius() * transform.scale.x.abs().max(transform.scale.y.abs());
            v.sees_disc(transform.position, radius)
        }
        None => true,
    }
}

/// Compute the (sub_size, uv, sub_anchor) tuples needed to render a sprite
/// with its [`SpriteFill`] mode and invoke `emit` for each sub-quad. Stretch
/// (or `None`) emits a single tuple matching the legacy single-quad path.
///
/// All sub-quads are emitted in the sprite's **local frame** with `sub_anchor`
/// chosen so that the batch's anchor-relative corner math places each cell at
/// the correct local-space rect; the caller then passes `transform.position`
/// and `transform.rotation` unchanged, so rotation pivots around the sprite's
/// main anchor and all cells rotate together as one rigid quad mesh.
#[inline]
fn for_each_fill_quad(
    sprite: &SpriteComponent,
    default_size: Vec2,
    scale: Vec2,
    mut emit: impl FnMut(Vec2, [f32; 4], [f32; 2]),
) {
    let base = sprite.drawn_size(default_size);
    let size = base * scale;
    let uv = sprite.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
    let anchor = sprite.anchor;

    match sprite.fill.as_ref() {
        None | Some(SpriteFill::Stretch) => {
            emit(size, uv, anchor);
        }
        Some(SpriteFill::NineSlice { left, right, top, bottom, source_px }) => {
            emit_nine_slice(size, uv, anchor, [*left, *right, *top, *bottom], *source_px, &mut emit);
        }
        Some(SpriteFill::Tile { tile_size }) => {
            emit_tile(size, uv, anchor, Vec2::new(tile_size[0], tile_size[1]), &mut emit);
        }
        Some(SpriteFill::Bar { amount, from }) => {
            let ((x0, x1, y0, y1), cut) = crate::ecs::sprite_fill::bar(size, anchor, uv, *amount, *from);
            if x1 > x0 && y1 > y0 {
                let (sub, sub_anchor) = rect_to_size_anchor(x0, x1, y0, y1);
                emit(sub, cut, sub_anchor);
            }
        }
        // The sprite pass draws a radial fill as a fan (`queue_sprite_tinted`);
        // a pass that draws only rectangles draws it whole.
        Some(SpriteFill::Radial { .. }) => {
            emit(size, uv, anchor);
        }
    }
}

/// Convert a local-space rect `[x0,x1] × [y0,y1]` (with local +Y "up", i.e.
/// `y1 > y0` for a normal cell) into the `(sub_size, sub_anchor)` pair that
/// makes [`WgpuSpriteBatch::draw_sprite_anchored`] place the cell at exactly
/// that local rect before rotation/translation.
#[inline]
fn rect_to_size_anchor(x0: f32, x1: f32, y0: f32, y1: f32) -> (Vec2, [f32; 2]) {
    let sw = x1 - x0;
    let sh = y1 - y0;
    let ax = if sw != 0.0 { -x0 / sw } else { 0.0 };
    let ay = if sh != 0.0 { -y0 / sh } else { 0.0 };
    (Vec2::new(sw, sh), [ax, ay])
}

/// `borders` is `[left, right, top, bottom]` in world units. `source_px` is
/// the pixel size of the sampled texture area: with it, a border of `n`
/// samples the outer `n` texture pixels, which is what makes corners hold
/// their shape. Without it (texture not loaded yet) the cells sample in
/// proportion to the drawn size - identical to a stretch.
fn emit_nine_slice(
    size: Vec2,
    uv: [f32; 4],
    anchor: [f32; 2],
    borders: [f32; 4],
    source_px: Option<[f32; 2]>,
    emit: &mut impl FnMut(Vec2, [f32; 4], [f32; 2]),
) {
    let [mut l, mut r, mut t, mut b] = borders;
    // UV insets as fractions of the sampled span, fixed before the on-screen
    // clamp below so a sprite drawn smaller than its borders still samples
    // the whole border.
    let src = source_px.unwrap_or([size.x, size.y]);
    let frac = |n: f32, span: f32| (n / span.max(1e-6)).clamp(0.0, 1.0);
    let (mut fl, mut fr, mut ft, mut fb) = (frac(l, src[0]), frac(r, src[0]), frac(t, src[1]), frac(b, src[1]));
    if fl + fr > 1.0 {
        let k = 1.0 / (fl + fr);
        fl *= k;
        fr *= k;
    }
    if ft + fb > 1.0 {
        let k = 1.0 / (ft + fb);
        ft *= k;
        fb *= k;
    }
    // Clamp borders so opposite pairs never exceed the sprite size — falls
    // back gracefully to a stretched look when borders are too thick.
    if l + r > size.x {
        let k = size.x / (l + r).max(1e-6);
        l *= k;
        r *= k;
    }
    if t + b > size.y {
        let k = size.y / (t + b).max(1e-6);
        t *= k;
        b *= k;
    }

    // Local x coords: left edge at -anchor.x*W, right at (1-anchor.x)*W.
    let x_min = -anchor[0] * size.x;
    let x_max = (1.0 - anchor[0]) * size.x;
    let y_min = -anchor[1] * size.y;
    let y_max = (1.0 - anchor[1]) * size.y;

    let xs = [x_min, x_min + l, x_max - r, x_max];
    // Local +Y is "up" / "top of quad", so y_max is the top edge. We list
    // y-stops top→bottom so the per-row UV slicing reads naturally.
    let ys_top_down = [y_max, y_max - t, y_min + b, y_min];

    // UV runs from (u0,v0)=top-left to (u1,v1)=bottom-right. Slice fractions
    // mirror the size fractions so borders match proportionally.
    let (u0, v0, u1, v1) = (uv[0], uv[1], uv[2], uv[3]);
    let u_w = u1 - u0;
    let v_h = v1 - v0;
    let us = [u0, u0 + fl * u_w, u1 - fr * u_w, u1];
    let vs_top_down = [v0, v0 + ft * v_h, v1 - fb * v_h, v1];

    for row in 0..3 {
        let y_top = ys_top_down[row];
        let y_bot = ys_top_down[row + 1];
        let v_top = vs_top_down[row];
        let v_bot = vs_top_down[row + 1];
        // Skip degenerate rows (e.g. zero-height middle when t+b == size.y).
        if (y_top - y_bot).abs() < 1e-6 {
            continue;
        }
        for col in 0..3 {
            let x_l = xs[col];
            let x_r = xs[col + 1];
            let u_l = us[col];
            let u_r = us[col + 1];
            if (x_r - x_l).abs() < 1e-6 {
                continue;
            }
            // Pass local rect: y0 = bottom (low y), y1 = top (high y).
            let (sub_size, sub_anchor) = rect_to_size_anchor(x_l, x_r, y_bot, y_top);
            emit(sub_size, [u_l, v_top, u_r, v_bot], sub_anchor);
        }
    }
}

fn emit_tile(
    size: Vec2,
    uv: [f32; 4],
    anchor: [f32; 2],
    tile_size: Vec2,
    emit: &mut impl FnMut(Vec2, [f32; 4], [f32; 2]),
) {
    // Defensive: a zero/negative tile dimension would loop forever; fall back
    // to a single stretched quad so the entity is still visible.
    if tile_size.x <= 0.0 || tile_size.y <= 0.0 || size.x <= 0.0 || size.y <= 0.0 {
        emit(size, uv, anchor);
        return;
    }

    let x_min = -anchor[0] * size.x;
    let y_min = -anchor[1] * size.y;
    let (u0, v0, u1, v1) = (uv[0], uv[1], uv[2], uv[3]);
    let u_w = u1 - u0;
    let v_h = v1 - v0;

    let cols = (size.x / tile_size.x).ceil() as i32;
    let rows = (size.y / tile_size.y).ceil() as i32;

    for j in 0..rows {
        // Tiles laid top→down: row 0 is the top row (highest y).
        let y_top_local = (1.0 - anchor[1]) * size.y - j as f32 * tile_size.y;
        let y_bot_local = (y_top_local - tile_size.y).max(y_min);
        let h_used = y_top_local - y_bot_local;
        let v_frac = h_used / tile_size.y;
        let v_top = v0;
        let v_bot = v0 + v_frac * v_h;
        for i in 0..cols {
            let x_l_local = x_min + i as f32 * tile_size.x;
            let x_r_local = (x_l_local + tile_size.x).min(-anchor[0] * size.x + size.x);
            let w_used = x_r_local - x_l_local;
            let u_frac = w_used / tile_size.x;
            let u_l = u0;
            let u_r = u0 + u_frac * u_w;
            let (sub_size, sub_anchor) =
                rect_to_size_anchor(x_l_local, x_r_local, y_bot_local, y_top_local);
            emit(sub_size, [u_l, v_top, u_r, v_bot], sub_anchor);
        }
    }
}

/// A material id that used to opt an entity into the lit-sprite **forward**
/// pass. Since Sep 2026 it is an ordinary sprite - drawn, sorted and blended
/// with everything else - whose `normal_map_id` shades it through the
/// deferred lights; `normal_map_id` works on any material now, so this name
/// is only kept so existing scenes mean what they meant.
pub const LIT_SPRITE_MATERIAL_ID: &str = "lit_sprite";

/// Whether anything in the world has a normal map - the one case the normal
/// pass has work to do. Without one, `WgpuLightingPass::render_lights`
/// clears the normal buffer flat itself and every light behaves as it
/// always has.
/// A frame's world targets for `camera`, and the camera to draw the world
/// through. A pixel-perfect camera (`pixel_size` > 0,
/// [`crate::renderer::pixel_perfect`]) gets targets of one texel per art
/// pixel, a camera snapped to the art's grid, and the output pass told which
/// part of the target to show; any other camera gets targets the size of
/// `surface` and is returned as it is. Every host calls this once a frame,
/// before drawing the world - the one place the targets are sized for it.
pub fn apply_pixel_plan(
    camera: crate::renderer::camera::Camera2D,
    surface: (u32, u32),
    device: &wgpu::Device,
    post: &mut crate::renderer::backend::wgpu::WgpuPostProcessStack,
    light: &mut crate::renderer::backend::wgpu::WgpuLightingPass,
) -> crate::renderer::camera::Camera2D {
    let plan = crate::renderer::pixel_perfect::plan(&camera);
    let (w, h) = plan.map_or(surface, |p| p.size);
    post.resize(device, w, h);
    light.resize(device, w, h);
    post.set_source(plan.map(|p| p.source));
    // The frame's colour grade, if it changed (`renderer::lut`).
    post.sync_lut(device);
    plan.map_or(camera, |p| p.camera)
}

/// A view's own world buffers (`ecs::views`): a post stack and a lighting
/// pass of its size, so a view is drawn by the same world pass as the screen
/// - lit, composited, post-processed - into its texture.
pub struct ViewTargets {
    pub post: crate::renderer::backend::wgpu::WgpuPostProcessStack,
    pub light: crate::renderer::backend::wgpu::WgpuLightingPass,
}

/// What a view texture is stored as: sRGB, as every colour texture is, so a
/// sprite sampling it reads linear colour like any other.
pub const VIEW_TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// Make, resize and drop the views' buffers and textures to match `views`.
/// Each view's post chain takes the scene's settings (`settings`), so a view
/// is graded, bloomed and toned as the screen is. Every host calls it once a
/// frame before drawing the views, then draws each through
/// [`apply_pixel_plan`] with its own `post` and `light`.
pub fn sync_view_targets(
    targets: &mut std::collections::HashMap<String, ViewTargets>,
    views: &[crate::ecs::views::ViewCamera],
    device: &wgpu::Device,
    assets: &mut crate::asset::AssetManager,
    scene_format: wgpu::TextureFormat,
    settings: &crate::renderer::postprocess::PostProcessSettings,
) {
    use crate::renderer::backend::wgpu::{WgpuLightingPass, WgpuPostProcessStack};
    targets.retain(|id, _| {
        let keep = views.iter().any(|v| &v.target == id);
        if !keep {
            assets.remove_wgpu_texture(id);
        }
        keep
    });
    for v in views {
        let (w, h) = v.size;
        assets.ensure_render_texture(&v.target, w, h, VIEW_TARGET_FORMAT, device);
        if !targets.contains_key(&v.target) {
            let post = match WgpuPostProcessStack::new(device, w, h, scene_format, VIEW_TARGET_FORMAT) {
                Ok(p) => p,
                Err(e) => {
                    log::error!("view '{}': no post stack: {e:#}", v.target);
                    continue;
                }
            };
            let light = match WgpuLightingPass::new(device, w, h, scene_format) {
                Ok(l) => l,
                Err(e) => {
                    log::error!("view '{}': no lighting pass: {e:#}", v.target);
                    continue;
                }
            };
            targets.insert(v.target.clone(), ViewTargets { post, light });
        }
        if let Some(t) = targets.get_mut(&v.target) {
            // A canvas group's picture is composited into the world, which
            // then has the post chain: run it here too and it would bloom
            // and tonemap twice.
            if v.transparent {
                t.post.set_settings(crate::renderer::postprocess::PostProcessSettings::default());
            } else {
                t.post.set_settings(settings.clone());
            }
        }
    }
}

/// Whether the camera this pass draws through draws `entity`: its
/// `RenderLayers` (layer 1 when it has none) share a bit with the frame
/// view's `cull_mask`. Free when the camera draws every layer, which is the
/// default; every world collector asks it, so a host needs nothing but the
/// `set_frame_view` it already calls per camera.
pub fn in_view_layers(world: &World, entity: hecs::Entity) -> bool {
    // Out of every fog's sight (`ecs::fog_of_war`): drawn by no pass.
    #[cfg(feature = "fog_of_war")]
    if crate::ecs::fog_of_war::hidden(world, entity) {
        return false;
    }
    let Some(view) = frame_view() else { return true };
    // Canvas groups (`ecs::canvas_group`): a group's own pass draws only its
    // members, the main pass none of them (it draws their picture), and any
    // other view everything.
    use crate::renderer::camera::CameraPass;
    match view.pass {
        CameraPass::CanvasGroup(g) => {
            if crate::ecs::canvas_group::member_of(entity) != g {
                return false;
            }
        }
        CameraPass::Main => {
            if crate::ecs::canvas_group::member_of(entity) != 0 {
                return false;
            }
        }
        CameraPass::View | CameraPass::Mirror(_) => {}
    }
    if view.cull_mask == u32::MAX {
        return true;
    }
    let layers = world.get::<&crate::ecs::components::RenderLayers>(entity).map_or(1, |l| l.layers);
    layers & view.cull_mask != 0
}

pub fn has_normal_maps(world: &World) -> bool {
    world
        .query::<&MaterialComponent>()
        .iter()
        .any(|(_, m)| m.normal_map_id.as_deref().is_some_and(|id| !id.is_empty()))
}

/// Does this frame need the normal pass? For a normal map, for a shiny
/// material (`specular`, which the pass writes beside the normals), or for
/// an occluder that spares other casters (`shadows_casters` off): the lights
/// know a caster's pixels only from the marks this pass writes, and a scene
/// of flat sprites used to skip it and shadow every caster anyway.
pub fn needs_normal_pass(world: &World) -> bool {
    has_normal_maps(world)
        // A light that leaves some layer out: every sprite must say which
        // layers it is on (`LightLayers`), and only this pass can.
        || uses_light_layers(world)
        || world.query::<&MaterialComponent>().iter().any(|(_, m)| m.specular > 0.0)
        || world
            .query::<&crate::ecs::components::LightOccluder>()
            .iter()
            .any(|(_, o)| o.enabled && !o.shadows_casters)
}

/// Whether some light lights fewer than every light layer (its `light_mask`
/// leaves out one of the eight the layer buffer holds) - the only case in
/// which sprites' `LightLayers` change anything.
pub fn uses_light_layers(world: &World) -> bool {
    use crate::ecs::components::{DirectionalLight2D, PointLight2D, SpotLight2D};
    let partial = |m: u32| m & 0xFF != 0xFF;
    world.query::<&PointLight2D>().iter().any(|(_, l)| partial(l.light_mask))
        || world.query::<&SpotLight2D>().iter().any(|(_, l)| partial(l.light_mask))
        || world.query::<&DirectionalLight2D>().iter().any(|(_, l)| partial(l.light_mask))
}

/// Draw every lit sprite's surface normal into the normal buffer, in draw
/// order: a sprite with `normal_map_id` writes its normal map, one without
/// writes flat, each covering what is under it by its alpha. The point and
/// spot lights read the result (`normal_response` in their shaders).
///
/// This is what lets a normal-mapped sprite sort with everything else.
/// Normal maps used to have a forward pass of their own (`lit_sprite`),
/// drawn after every other sprite, so a normal-mapped floor covered the
/// fire, the particles and the props standing on it. Now the colour is drawn
/// in the ordinary sprite pass and only the *normal* is drawn here - and
/// since flat sprites overwrite it too, a crate on that floor hides the
/// floor's bumps.
///
/// `flush_group(pipeline, diffuse_id, normal_id)` resolves the textures
/// (`lit_sprite_textures`, with the flat fallback for `None`) and calls
/// `pipeline.flush_normals`. `Unlit` sprites are skipped: nothing lights
/// them. Meshes are drawn too (Oct 2026), in perspective. Tilemaps, text and
/// particles are not, so they take the normal of whatever is under them -
/// flat, unless it is bumpy.
pub fn render_normals_wgpu(
    world:        &World,
    sort_layers:  &SortLayers,
    pipeline:     &mut WgpuLitSpritePipeline,
    default_size: Vec2,
    mut flush_group: impl FnMut(&mut WgpuLitSpritePipeline, &str, Option<&str>),
) -> usize {
    let view = frame_view();
    let mut query = world.query::<(
        &Transform,
        &SpriteComponent,
        Option<&MaterialComponent>,
        Option<&WorldTransform>,
        Option<&crate::ecs::components::Unlit>,
        Option<&crate::ecs::components::LightOccluder>,
        Option<&crate::ecs::components::LightLayers>,
        Option<&PaintableC>,
    )>();
    // Only what covers what is under it is a surface. An additive glow or a
    // multiply decal (a scorch mark) tints the floor and has no shape of its
    // own; drawn here it wrote a flat normal over the floor's bumps, and the
    // floor under every scorch lit as a pale rectangle.
    let covers = |m: Option<&MaterialComponent>| {
        matches!(
            sprite_blend(m),
            crate::renderer::material::BlendMode::Alpha | crate::renderer::material::BlendMode::PremultipliedAlpha
        )
    };
    let mut drawables: Vec<NormalSurface> = query
        .iter()
        .filter(|(e, (_, s, m, _, unlit, _, _, _))| {
            unlit.is_none() && in_view_layers(world, *e) && s.color[3] > 0.0 && covers(*m)
        })
        // A sprite whose entity casts a shadow is marked as a caster, for
        // occluders that do not shadow casters (`shadows_casters` off).
        .map(|(_, (t, s, m, w, _, occ, layers, pressed))| {
            let marks = (occ.is_some_and(|o| o.enabled), layers.map_or(1, |l| l.layers));
            // A pressed sprite's normals come from its pressed copy.
            let pressed = crate::ecs::carve::pressed_normal(pressed);
            (w.cloned().unwrap_or_else(|| WorldTransform::from(t)), s, m, marks, pressed)
        })
        .filter(|(transform, s, _, _, _)| {
            view.is_none_or(|v| v.sees_sprite(transform.position, transform.scale, s, default_size))
        })
        .map(|(transform, s, m, marks, pressed)| NormalSurface {
            z: sort_layers.resolve(s.sort_layer.as_deref(), s.z_order),
            y_key: s.y_sort_key(transform.position.y),
            tex: crate::asset::resolve_texture_id(&s.texture_id),
            normal: pressed.or_else(|| m.and_then(|m| m.normal_map_id.as_deref()).filter(|id| !id.is_empty())),
            marks,
            specular: m.map_or([0.0, 0.0], |m| specular_encoding(m.specular, m.shininess)),
            transform,
            shape: NormalShape::Sprite(s),
        })
        .collect();
    // Meshes are surfaces too (Oct 2026): a normal-mapped wall drawn as a
    // mesh - a 3D stage's, a corridor's - lit flat before, whatever its map.
    let mut mesh_query = world.query::<(
        &Transform,
        &Mesh2D,
        Option<&MaterialComponent>,
        Option<&WorldTransform>,
        Option<&crate::ecs::components::Unlit>,
        Option<&crate::ecs::components::LightOccluder>,
        Option<&crate::ecs::components::LightLayers>,
    )>();
    for (e, (t, mesh, m, w, unlit, occ, layers)) in mesh_query.iter() {
        if unlit.is_some() || !in_view_layers(world, e) || mesh.color[3] <= 0.0 || mesh.vertices.len() < 3 || !covers(m) {
            continue;
        }
        let transform = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        if !view_sees_mesh(view.as_ref(), &transform, mesh) {
            continue;
        }
        drawables.push(NormalSurface {
            z: sort_layers.resolve(mesh.sort_layer.as_deref(), mesh.z_order),
            y_key: 0,
            tex: crate::asset::resolve_texture_id(&mesh.texture_id),
            normal: m.and_then(|m| m.normal_map_id.as_deref()).filter(|id| !id.is_empty()),
            marks: (occ.is_some_and(|o| o.enabled), layers.map_or(1, |l| l.layers)),
            specular: m.map_or([0.0, 0.0], |m| specular_encoding(m.specular, m.shininess)),
            transform,
            shape: NormalShape::Mesh(mesh),
        });
    }
    if drawables.is_empty() {
        return 0;
    }

    // The colour pass's order: z, then down the screen for whoever asked,
    // then by texture pair so neighbours flush together.
    drawables.sort_by(|a, b| {
        a.z.cmp(&b.z).then_with(|| a.y_key.cmp(&b.y_key)).then_with(|| a.tex.cmp(b.tex)).then_with(|| a.normal.cmp(&b.normal))
    });

    let mut total = 0usize;
    let mut i = 0;
    while i < drawables.len() {
        let diffuse_id = drawables[i].tex;
        let normal_id = drawables[i].normal;
        pipeline.begin();
        let mut count = 0usize;
        while i < drawables.len() && drawables[i].tex == diffuse_id && drawables[i].normal == normal_id {
            if pipeline.is_full() {
                flush_group(pipeline, diffuse_id, normal_id);
                total += count;
                count = 0;
                pipeline.begin();
            }
            let d = &drawables[i];
            match d.shape {
                NormalShape::Sprite(s) => queue_lit_sprite(pipeline, &d.transform, s, default_size, d.marks, d.specular),
                NormalShape::Mesh(mesh) => {
                    let (positions, uvq, indices) = mesh_geometry(&d.transform, mesh);
                    let color = Vec4::new(normal_code(d.marks), d.specular[0], d.specular[1], mesh.color[3]);
                    // A mesh longer than a batch: flush and draw the rest.
                    let mut rest = indices.as_slice();
                    loop {
                        let left = pipeline.draw_mesh(&positions, &uvq, color, rest);
                        if left == 0 {
                            break;
                        }
                        flush_group(pipeline, diffuse_id, normal_id);
                        total += count;
                        count = 0;
                        pipeline.begin();
                        rest = &rest[rest.len() - left * 3..];
                    }
                }
            }
            count += 1;
            i += 1;
        }
        flush_group(pipeline, diffuse_id, normal_id);
        total += count;
    }
    total
}

/// One surface the normal pass draws: where it sorts, its two textures, the
/// code its vertices carry, and its shape.
struct NormalSurface<'a> {
    z: i32,
    y_key: i32,
    tex: &'a str,
    normal: Option<&'a str>,
    /// Whether it is a caster, and its light layers.
    marks: (bool, u32),
    specular: [f32; 2],
    transform: WorldTransform,
    shape: NormalShape<'a>,
}

enum NormalShape<'a> {
    Sprite(&'a SpriteComponent),
    Mesh(&'a Mesh2D),
}

/// The red of a normal-pass vertex: its light layers and whether it is a
/// caster, `layers * 2 + (1 if not)` - an integer, exact in the vertex's
/// float (`fs_normal` decodes it).
fn normal_code((caster, layers): (bool, u32)) -> f32 {
    ((layers & 0xFF) * 2 + if caster { 0 } else { 1 }) as f32
}

/// A [`Mesh2D`]'s triangles as the sprite pass draws them: positions carried
/// by the world transform, `(u q, v q, q)` inside its `uv_rect`, and its
/// triangle list.
fn mesh_geometry(transform: &WorldTransform, mesh: &Mesh2D) -> (Vec<Vec2>, Vec<[f32; 3]>, Vec<u32>) {
    let [u0, v0, u1, v1] = mesh.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
    let mut positions = Vec::with_capacity(mesh.vertices.len());
    let mut uvq = Vec::with_capacity(mesh.vertices.len());
    for v in &mesh.vertices {
        positions.push(transform.apply(Vec2::from(v.position)));
        let q = if v.q.is_finite() && v.q > 0.0 { v.q } else { 1.0 };
        let u = u0 + (u1 - u0) * v.uv[0];
        let vv = v0 + (v1 - v0) * v.uv[1];
        uvq.push([u * q, vv * q, q]);
    }
    (positions, uvq, mesh.triangles().into_owned())
}

/// What the velocity pass remembers between frames: the camera's view, when,
/// and where each sprite was (`motion_blur`).
#[derive(Default)]
struct VelocityHistory {
    corners: Option<[Vec2; 4]>,
    at: f64,
    positions: std::collections::HashMap<u64, Vec2>,
    /// The floating origin the history last followed.
    origin_seen: Option<(f64, f64)>,
}

thread_local! {
    static VELOCITY: std::cell::RefCell<VelocityHistory> = std::cell::RefCell::new(VelocityHistory::default());
}

/// Where world point `p` falls on a view whose world corners are `corners`
/// (top-left, top-right, bottom-right, bottom-left - `Camera2D::view_corners`),
/// in screen UV, y down.
pub fn view_uv(corners: &[Vec2; 4], p: Vec2) -> Vec2 {
    let (tl, tr, bl) = (corners[0], corners[1], corners[3]);
    let (ax, ay) = (tr - tl, bl - tl);
    let d = p - tl;
    Vec2::new(d.dot(ax) / ax.length_squared().max(1e-6), d.dot(ay) / ay.length_squared().max(1e-6))
}

/// Start the velocity pass for `camera`: the motion across the screen of a
/// thing standing still (the camera's own - what the target is cleared to),
/// and the scale from this frame's motion to a sixtieth of a second's, so a
/// blur is the same length at any frame rate.
pub fn begin_velocity(camera: &crate::renderer::camera::Camera2D) -> ([f32; 2], f32) {
    // A monotonic clock: the wall clock ticks in steps of up to 16 ms on
    // Windows, and a frame's time read off it swung between nothing (the
    // blur eight times too long) and a whole step (too short to see).
    let now = crate::profiling::now_ms() / 1000.0;
    let corners = camera.view_corners();
    VELOCITY.with(|v| {
        let mut v = v.borrow_mut();
        // The world was moved back (`ecs::floating_origin`): so is what the
        // last frame saw, or everything would streak across the jump.
        let by = crate::ecs::floating_origin::follow(&mut v.origin_seen);
        if by != Vec2::ZERO {
            if let Some(c) = v.corners.as_mut() {
                for p in c.iter_mut() {
                    *p -= by;
                }
            }
            for p in v.positions.values_mut() {
                *p -= by;
            }
        }
        let dt = if v.at > 0.0 { (now - v.at) as f32 } else { 1.0 / 60.0 };
        let scale = (1.0 / 60.0) / dt.clamp(1.0 / 480.0, 1.0 / 10.0);
        let Some(prev) = v.corners else { return ([0.0, 0.0], scale) };
        let centre = (corners[0] + corners[2]) * 0.5;
        let vel = (Vec2::splat(0.5) - view_uv(&prev, centre)) * scale;
        ([vel.x, vel.y], scale)
    })
}

/// The velocity pass: every covering sprite and mesh drawn with its motion
/// across the screen since the last frame - its own and the camera's - in the
/// vertex colour's red and green, and its z in its blue (`depth_of_field`
/// reads it; a 3D stage's objects give their depth instead, per vertex for a
/// quad), through `WgpuSpriteBatch::velocity_pipeline`. A sprite the camera
/// follows has none and stays sharp under `motion_blur`; the street it
/// drives down streams past. A sprite seen for the first time has none.
/// Remembers this frame for the next.
pub fn render_velocity_wgpu(
    world: &World,
    sort_layers: &SortLayers,
    batch: &mut WgpuSpriteBatch,
    default_size: Vec2,
    camera: &crate::renderer::camera::Camera2D,
    scale: f32,
    mut flush: impl FnMut(&mut WgpuSpriteBatch, &str),
) -> usize {
    let corners = camera.view_corners();
    let view = frame_view();
    let (prev_corners, prev_positions) =
        VELOCITY.with(|v| { let mut v = v.borrow_mut(); (v.corners, std::mem::take(&mut v.positions)) });
    let mut seen: std::collections::HashMap<u64, Vec2> = std::collections::HashMap::with_capacity(prev_positions.len());
    let covers = |m: Option<&MaterialComponent>| {
        matches!(
            sprite_blend(m),
            crate::renderer::material::BlendMode::Alpha | crate::renderer::material::BlendMode::PremultipliedAlpha
        )
    };
    // The motion of a thing at `position` since the last frame, across the
    // screen.
    let mut motion = |e: hecs::Entity, position: Vec2| {
        let key = e.to_bits().get();
        seen.insert(key, position);
        let was = prev_positions.get(&key).copied().unwrap_or(position);
        match prev_corners {
            Some(pc) if prev_positions.contains_key(&key) => (view_uv(&corners, position) - view_uv(&pc, was)) * scale,
            _ => Vec2::ZERO,
        }
    };
    let mut items: Vec<VelocityItem> = Vec::new();
    let mut query = world.query::<(
        &Transform,
        &SpriteComponent,
        Option<&WorldTransform>,
        Option<&MaterialComponent>,
        Option<&StageBillboardC>,
    )>();
    for (e, (t, s, w, m, billboard)) in query.iter() {
        if s.color[3] <= 0.0 || !in_view_layers(world, e) || !covers(m) {
            continue;
        }
        let wt = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        let vel = motion(e, wt.position);
        if !view.is_none_or(|v| v.sees_sprite(wt.position, wt.scale, s, default_size)) {
            continue;
        }
        let z = sort_layers.resolve(s.sort_layer.as_deref(), s.z_order);
        // A thing standing on a 3D stage is as deep as the stage says
        // (`ecs::stage`), not as its z - which only orders it.
        #[cfg(feature = "stage")]
        let depth = billboard.map_or(z as f32, |b| b.view_z);
        #[cfg(not(feature = "stage"))]
        let depth = {
            let _ = billboard;
            z as f32
        };
        items.push(VelocityItem {
            z,
            y_key: s.y_sort_key(wt.position.y),
            tex: crate::asset::resolve_texture_id(&s.texture_id),
            color: Vec4::new(vel.x, vel.y, depth, s.color[3]),
            transform: wt,
            shape: VelocityShape::Sprite(s),
        });
    }
    // Meshes (Oct 2026): a 3D stage's floors and walls blur by their depth,
    // which a quad carries per vertex - so a floor softens smoothly into the
    // distance instead of as one layer.
    let mut mesh_query = world.query::<(
        &Transform,
        &Mesh2D,
        Option<&WorldTransform>,
        Option<&MaterialComponent>,
        Option<&StageQuadC>,
    )>();
    for (e, (t, mesh, w, m, quad)) in mesh_query.iter() {
        if mesh.color[3] <= 0.0 || mesh.vertices.len() < 3 || !in_view_layers(world, e) || !covers(m) {
            continue;
        }
        let wt = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        let vel = motion(e, wt.position);
        if !view_sees_mesh(view.as_ref(), &wt, mesh) {
            continue;
        }
        let z = sort_layers.resolve(mesh.sort_layer.as_deref(), mesh.z_order);
        #[cfg(feature = "stage")]
        let depths = quad.map(|q| q.view_z.as_slice()).filter(|d| d.len() == mesh.vertices.len());
        #[cfg(not(feature = "stage"))]
        let depths: Option<&[f32]> = {
            let _ = quad;
            None
        };
        items.push(VelocityItem {
            z,
            y_key: 0,
            tex: crate::asset::resolve_texture_id(&mesh.texture_id),
            color: Vec4::new(vel.x, vel.y, z as f32, mesh.color[3]),
            transform: wt,
            shape: VelocityShape::Mesh(mesh, depths),
        });
    }
    // Nearer drawn last, so it is what a pixel's motion is.
    items.sort_by(|a, b| a.z.cmp(&b.z).then_with(|| a.y_key.cmp(&b.y_key)));
    let mut total = 0usize;
    let mut i = 0;
    while i < items.len() {
        let tex = items[i].tex;
        batch.begin();
        let mut count = 0usize;
        while i < items.len() && items[i].tex == tex {
            if batch.is_full() {
                flush(batch, tex);
                total += count;
                count = 0;
                batch.begin();
            }
            let it = &items[i];
            match it.shape {
                VelocityShape::Sprite(s) => {
                    let wt = &it.transform;
                    for_each_fill_quad(s, default_size, wt.scale, |sub_size, uv, sub_anchor| {
                        batch.draw_sprite_anchored_skewed(wt.position, sub_size, wt.rotation, wt.shear, it.color, uv, sub_anchor);
                    });
                }
                VelocityShape::Mesh(mesh, depths) => {
                    let (positions, uvq, indices) = mesh_geometry(&it.transform, mesh);
                    let colors: Vec<[f32; 4]> = (0..positions.len())
                        .map(|k| {
                            let mut c = it.color;
                            if let Some(d) = depths {
                                // Times the vertex's perspective weight: the
                                // shader divides by the weight it is handed,
                                // as it does the texture coordinate, so the
                                // depth is right across a quad in depth.
                                c.z = d[k] * uvq[k][2];
                            }
                            // Coverage: the vertex's own alpha too.
                            c.w *= mesh.vertices[k].color[3];
                            c.into()
                        })
                        .collect();
                    batch.draw_mesh(&positions, &uvq, &colors, &indices);
                    while batch.has_overflow() {
                        let rest = batch.take_overflow();
                        flush(batch, tex);
                        total += count;
                        count = 0;
                        batch.begin();
                        batch.resume_overflow(rest);
                    }
                }
            }
            count += 1;
            i += 1;
        }
        flush(batch, tex);
        total += count;
    }
    VELOCITY.with(|v| {
        let mut v = v.borrow_mut();
        v.positions = seen;
        v.corners = Some(corners);
        v.at = crate::profiling::now_ms() / 1000.0;
    });
    total
}

/// One thing the velocity pass draws: where it sorts, its texture, the
/// colour its vertices carry (motion, depth, coverage) and its shape.
struct VelocityItem<'a> {
    z: i32,
    y_key: i32,
    tex: &'a str,
    color: Vec4,
    transform: WorldTransform,
    shape: VelocityShape<'a>,
}

enum VelocityShape<'a> {
    Sprite(&'a SpriteComponent),
    /// A mesh, and its depth per vertex when a 3D stage gave it one.
    Mesh(&'a Mesh2D, Option<&'a [f32]>),
}

/// A material's specular as the normal pass carries it in a vertex colour:
/// the strength (0..1) and the shininess as 0..1 of 1..128
/// (`light_specular.wgsl` decodes it).
pub fn specular_encoding(specular: f32, shininess: f32) -> [f32; 2] {
    [specular.clamp(0.0, 1.0), ((shininess - 1.0) / 127.0).clamp(0.0, 1.0)]
}

#[inline]
fn queue_lit_sprite(
    pipeline:     &mut WgpuLitSpritePipeline,
    transform:    &WorldTransform,
    sprite:       &SpriteComponent,
    default_size: Vec2,
    (caster, layers): (bool, u32),
    specular:     [f32; 2],
) {
    // The normal pass reads the alpha for coverage; red carries the light
    // layers and whether this is a caster, `layers * 2 + (1 if not)` - an
    // integer, exact in the vertex's float - green and blue the material's
    // specular strength and shininess - see `fs_normal`.
    let color = Vec4::new(normal_code((caster, layers)), specular[0], specular[1], sprite.color[3]);
    for_each_fill_quad(sprite, default_size, transform.scale, |sub_size, uv, sub_anchor| {
        pipeline.draw_sprite_anchored_skewed(
            transform.position,
            sub_size,
            transform.rotation,
            transform.shear,
            color,
            uv,
            sub_anchor,
        );
    });
}

/// The diffuse and normal textures one lit-sprite group draws with.
///
/// Every host's `render_lit_sprites_wgpu` flush closure used to resolve these
/// itself, and the one host that did it at all got the normal map wrong: it
/// went through [`AssetManager::ensure_wgpu_texture`], which uploads sRGB, so
/// the shader gamma-decoded the vectors and every normal - flat ones included -
/// leaned some 40 degrees down-left. The lighting came out lopsided and faint.
/// Normal maps upload linear here, once, for everyone.
///
/// A material with no `normal_map_id` draws against `flat_normal`. One that
/// *names* a normal map whose image is not loaded does too, rather than being
/// dropped - but that case is a bug, not a default, so the third element names
/// the missing id for the host to warn about. Falling back silently is exactly
/// how the editor and web came to draw scene 12's bump row flat without anyone
/// being told: neither ever loaded the normal map's bytes.
///
/// `Err` means the *diffuse* could not be resolved, and carries the reason for
/// the host to log in its own voice; the group should be skipped.
pub fn lit_sprite_textures<'a>(
    assets: &'a mut crate::asset::AssetManager,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    diffuse_id: &str,
    normal_id: Option<&str>,
    flat_normal: &'a crate::renderer::backend::wgpu::WgpuTexture,
) -> Result<
    (
        &'a crate::renderer::backend::wgpu::WgpuTexture,
        &'a crate::renderer::backend::wgpu::WgpuTexture,
        Option<String>,
    ),
    String,
> {
    if !assets.has_texture_bytes(diffuse_id) && assets.get_wgpu_texture(diffuse_id).is_none() {
        return Err(format!("missing lit-sprite diffuse '{diffuse_id}'"));
    }
    assets
        .ensure_wgpu_texture(diffuse_id, device, queue)
        .map_err(|e| format!("failed to upload lit-sprite diffuse '{diffuse_id}': {e:#}"))?;
    if let Some(nid) = normal_id
        && assets.has_texture_bytes(nid)
    {
        // A normal map that fails to upload falls back to flat, below.
        let _ = assets.ensure_wgpu_normal_texture(nid, device, queue);
    }
    let assets: &'a crate::asset::AssetManager = assets;
    let diffuse = assets
        .get_wgpu_texture(diffuse_id)
        .expect("diffuse was just ensured");
    let resolved = normal_id.and_then(|nid| assets.get_wgpu_normal_texture(nid));
    let missing = match (normal_id, resolved) {
        (Some(nid), None) => Some(nid.to_string()),
        _ => None,
    };
    Ok((diffuse, resolved.unwrap_or(flat_normal), missing))
}

///
/// Mirrors the gl `render_shapes` body: lyon-tessellated fill + stroke,
/// transformed by the entity's `Transform`. Caller is responsible for
/// `batch.begin()` and `batch.flush(queue, pass, view_projection)`.
/// **Superseded** - vector paths are drawn by the sprite pass since Sep
/// 2026, sorted by `z_order` with everything else (`renderer::vector_path`).
/// No runtime calls this any more; it is kept, unchanged, for an outside
/// caller that drew paths into a `WgpuShapeBatch` of its own, and it would
/// draw every path a *second* time over the sprites if a runtime called it.
#[deprecated(note = "vector paths are drawn by the sprite pass; see renderer::vector_path")]
pub fn render_shapes_wgpu(world: &World, sort_layers: &SortLayers, batch: &mut WgpuShapeBatch) {
    use crate::ecs::components::PathCommand;
    use lyon::math::point;
    use lyon::path::Path;
    use lyon::tessellation::{
        BuffersBuilder, FillOptions, FillTessellator, StrokeOptions, StrokeTessellator,
        VertexBuffers,
    };

    let mut drawables: Vec<(Transform, VectorPath)> = world
        .query::<(&Transform, &VectorPath)>()
        .iter()
        .map(|(_, (t, vp))| (t.clone(), vp.clone()))
        .collect();
    drawables.sort_by_key(|(_, vp)| sort_layers.resolve(vp.sort_layer.as_deref(), vp.z_order));

    for (transform, vector_path) in &drawables {
        if vector_path.commands.is_empty() {
            continue;
        }

        let mut builder = Path::builder();
        // lyon requires strict begin/end pairing per sub-path. Track it
        // so back-to-back MoveTo, explicit Close commands, and the
        // optional `closed` flag never call end()/close() without an
        // open sub-path.
        let mut in_subpath = false;
        for cmd in &vector_path.commands {
            match cmd {
                PathCommand::MoveTo { x, y } => {
                    if in_subpath {
                        builder.end(false);
                    }
                    builder.begin(point(*x, *y));
                    in_subpath = true;
                }
                PathCommand::LineTo { x, y } => {
                    builder.line_to(point(*x, *y));
                }
                PathCommand::QuadTo { cx, cy, x, y } => {
                    builder.quadratic_bezier_to(point(*cx, *cy), point(*x, *y));
                }
                PathCommand::CubicTo { c1x, c1y, c2x, c2y, x, y } => {
                    builder.cubic_bezier_to(
                        point(*c1x, *c1y),
                        point(*c2x, *c2y),
                        point(*x, *y),
                    );
                }
                PathCommand::Close => {
                    if in_subpath {
                        builder.close();
                        in_subpath = false;
                    }
                }
            }
        }
        if in_subpath {
            if vector_path.closed {
                builder.close();
            } else {
                builder.end(false);
            }
        }
        let path = builder.build();

        if let Some(fill_color) = vector_path.fill_color {
            let mut geometry: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
            let mut tessellator = FillTessellator::new();
            let result = tessellator.tessellate_path(
                &path,
                &FillOptions::default(),
                &mut BuffersBuilder::new(&mut geometry, |v: lyon::tessellation::FillVertex| {
                    v.position().to_array()
                }),
            );
            if result.is_ok() && !geometry.indices.is_empty() {
                batch.draw_tessellated_transformed(
                    &geometry.vertices,
                    &geometry.indices,
                    fill_color,
                    transform.position,
                    transform.rotation,
                    transform.scale,
                );
            }
        }

        if let Some(stroke_color) = vector_path.stroke_color {
            let mut geometry: VertexBuffers<[f32; 2], u32> = VertexBuffers::new();
            let mut tessellator = StrokeTessellator::new();
            let result = tessellator.tessellate_path(
                &path,
                &StrokeOptions::default().with_line_width(vector_path.stroke_width),
                &mut BuffersBuilder::new(&mut geometry, |v: lyon::tessellation::StrokeVertex| {
                    v.position().to_array()
                }),
            );
            if result.is_ok() && !geometry.indices.is_empty() {
                batch.draw_tessellated_transformed(
                    &geometry.vertices,
                    &geometry.indices,
                    stroke_color,
                    transform.position,
                    transform.rotation,
                    transform.scale,
                );
            }
        }
    }
}

/// Queue every `Transform + TextComponent` entity onto `batch` as glyph quads,
/// grouped into one draw per font atlas.
///
/// Phase 3.c-4 Step 5a: wgpu twin of [`crate::ecs::systems::render_text`].
/// `fonts` resolves each entity's `TextComponent.font_id` to the glyph metrics
/// and the atlas holding them; an empty id means the built-in font. Because a
/// custom font brings its own texture, this function owns `batch.begin()` and
/// invokes `flush_group(batch, texture_id)` per group - the caller resolves
/// that id (typically via `AssetManager::ensure_wgpu_texture`) and flushes,
/// exactly as it already does for [`render_tilemaps_wgpu`].
///
/// Entities are sorted by resolved z, then split into **runs** of consecutive
/// entities sharing an atlas - not bucketed by font. Bucketing would reorder
/// text across fonts, letting a background label jump in front of foreground
/// text purely because the two used different faces. A run costs one extra
/// draw call only where fonts actually interleave in z.
///
/// Returns the number of glyph quads queued across all groups (0 if no entity
/// has non-empty text, in which case `flush_group` is never called).
///
/// Skips entities carrying [`Unlit`](crate::ecs::components::Unlit): the
/// world pass draws those, through its [`WorldLayer`]s.
pub fn render_text_wgpu(
    world: &World,
    sort_layers: &SortLayers,
    batch: &mut WgpuSpriteBatch,
    fonts: &FrameFonts<'_>,
    flush_group: impl FnMut(&mut WgpuSpriteBatch, &str),
) -> usize {
    render_text_filtered(world, sort_layers, batch, fonts, flush_group, false)
}

fn render_text_filtered(
    world: &World,
    sort_layers: &SortLayers,
    batch: &mut WgpuSpriteBatch,
    fonts: &FrameFonts<'_>,
    mut flush_group: impl FnMut(&mut WgpuSpriteBatch, &str),
    unlit: bool,
) -> usize {
    // Empty strings are dropped here rather than skipped in the loop below:
    // left in, an entity that renders nothing would still split a run in two
    // and cost a draw call.
    //
    // Placed by the **world** transform, like sprites: text parented to
    // another entity (a card's name on its card) used to be drawn at its
    // local position, as if it had no parent. Borrowed, not cloned: the
    // strings are not copied every frame.
    let mut query = world.query::<(
        &Transform,
        &TextComponent,
        Option<&WorldTransform>,
        Option<&crate::ecs::components::Unlit>,
    )>();
    let mut texts: Vec<((Vec2, f32, f32), &TextComponent)> = query
        .iter()
        .filter(|(_, (_, tc, _, u))| !tc.text.is_empty() && u.is_some() == unlit)
        .map(|(_, (t, tc, w, _))| {
            (w.map_or((t.position, t.scale.x, t.rotation), |w| (w.position, w.scale.x, w.rotation)), tc)
        })
        .collect();
    texts.sort_by_key(|(_, tc)| sort_layers.resolve(tc.sort_layer.as_deref(), tc.z_order));

    let mut total = 0usize;
    let mut i = 0usize;
    while i < texts.len() {
        let (_, group_tex) = fonts.resolve(&texts[i].1.font_id);
        batch.begin();
        let mut queued = 0usize;
        while i < texts.len() {
            let ((position, world_scale, rotation), text_comp) = texts[i];
            // Re-resolve per entity rather than reusing the group's font: two
            // ids can share an atlas while carrying different metrics, and the
            // glyph rects must come from the id that was actually asked for.
            let (font_data, tex) = fonts.resolve(&text_comp.font_id);
            if tex != group_tex {
                break;
            }
            let color = Vec4::from(text_comp.color);
            let scale = text_comp.font_scale * world_scale;
            // `wrap_width` is in world units, the same space the glyph quads
            // are laid out in; 0 means "no wrapping".
            let wrap = (text_comp.wrap_width > 0.0).then_some(text_comp.wrap_width);
            queued += queue_text_glyphs_rotated(
                font_data,
                batch,
                &text_comp.text,
                position,
                scale,
                rotation,
                color,
                wrap,
                text_comp.align,
            );
            i += 1;
        }
        if queued > 0 {
            flush_group(batch, group_tex);
            total += queued;
        }
    }
    total
}

/// Emit per-glyph quads for `text` into `batch`. Mirrors the layout math of
/// [`crate::renderer::text::BitmapFont::draw_text`] — same Y-up world-space
/// convention (TL origin, glyph centers placed below the cursor line).
///
/// Falls back to '?' for missing glyphs. **Honours newlines**: each line is
/// drawn one `line_height` lower than the last. Text used to be strictly
/// single-line, with newlines silently dropped, so multi-line output meant
/// stacking one entity per line by hand.
pub(crate) fn queue_text_glyphs(
    font_data: &FontData,
    batch: &mut WgpuSpriteBatch,
    text: &str,
    position: Vec2,
    scale: f32,
    color: Vec4,
) -> usize {
    queue_text_glyphs_wrapped(font_data, batch, text, position, scale, color, None, TextAlign::Left)
}

/// One text field's drawing this frame (`ui::text_edit`): the characters
/// that fit, where they go, the selection, the caret and an IME's
/// composition - shared by entity UI and `.gui` fields, which differ only in
/// how they flush.
pub(crate) struct FieldDraw {
    pub shown: String,
    pub text_pos: Vec2,
    pub color: Vec4,
    /// Centre and size of each quad, all drawn in `highlight`.
    pub selection: Option<(Vec2, Vec2)>,
    pub caret: Option<(Vec2, Vec2)>,
    pub underline: Option<(Vec2, Vec2)>,
}

/// Lay a field out and remember what was measured for the next frame's
/// clicks (`text_edit::set_view`). `left`, `bottom`, `width`, `height` are
/// the field in the renderer's Y-up space; `view_rect` is what to record
/// for it (each caller's own convention); `color` is already faded.
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan_text_field(
    fd: &FontData,
    text: &str,
    placeholder: &str,
    password: bool,
    edit: &crate::ui::text_edit::TextEdit,
    focused: bool,
    key: u64,
    [left, bottom, width, height]: [f32; 4],
    view_rect: [f32; 4],
    scale: f32,
    color: Vec4,
) -> FieldDraw {
    use crate::ui::text_edit as te;
    let pad = 6.0_f32.min(width * 0.25);
    let inner_left = left + pad;
    let inner_w = (width - 2.0 * pad).max(1.0);
    let center_y = bottom + height * 0.5;
    let line_h = fd.line_height * scale;
    let preedit = if focused { crate::input::text_entry::preedit() } else { String::new() };
    let base = if password { te::masked(text) } else { text.to_string() };
    let (display, comp) = te::with_preedit(&base, edit.caret, &preedit);
    let caret = comp.map_or(edit.caret, |(_, e)| e);
    let advance = |ch: char| {
        fd.glyphs
            .iter()
            .find(|g| g.ch == ch)
            .or_else(|| fd.glyphs.iter().find(|g| g.ch == '?'))
            .map_or(0.0, |g| g.advance * scale)
    };
    let mut advances = Vec::with_capacity(display.chars().count() + 1);
    let mut x = 0.0;
    advances.push(0.0);
    for ch in display.chars() {
        x += advance(ch);
        advances.push(x);
    }
    let now = crate::profiling::unix_time();
    let prev = te::view(key).map_or(0.0, |v| v.scroll);
    let lay = te::layout(
        &advances,
        caret,
        if comp.is_some() || !focused { None } else { edit.selection() },
        focused && edit.caret_visible(now),
        inner_left,
        inner_w,
        prev,
    );
    te::set_view(key, te::FieldView { rect: view_rect_with_pad(view_rect, pad), scroll: lay.scroll, advances: advances.clone() });
    let quad = |x0: f32, x1: f32, y: f32, h: f32| (Vec2::new((x0 + x1) * 0.5, y), Vec2::new((x1 - x0).max(0.0), h));
    let (shown, text_pos, color) = if display.is_empty() {
        let dim = Vec4::new(color.x, color.y, color.z, color.w * 0.45);
        (placeholder.to_string(), Vec2::new(inner_left, centered_line_top(fd, placeholder, scale, center_y)), dim)
    } else {
        let shown: String = display.chars().skip(lay.first).take(lay.last - lay.first).collect();
        (shown, Vec2::new(lay.text_x, centered_line_top(fd, "x", scale, center_y)), color)
    };
    let at = |i: usize| inner_left + advances.get(i).copied().unwrap_or(0.0) - lay.scroll;
    FieldDraw {
        shown,
        text_pos,
        color,
        selection: lay.selection.map(|(x0, x1)| quad(x0, x1, center_y, line_h)),
        caret: lay.caret_x.map(|cx| (Vec2::new(cx + 0.5, center_y), Vec2::new(2.0_f32.max(scale), line_h))),
        underline: comp.map(|(s, e)| {
            let (x0, x1) = (at(s).max(inner_left), at(e).min(inner_left + inner_w));
            quad(x0, x1, center_y - line_h * 0.5 + 1.0, 1.5)
        }),
    }
}

/// The text area inside a field's rectangle, which is what clicks map onto.
fn view_rect_with_pad([x, y, w, h]: [f32; 4], pad: f32) -> [f32; 4] {
    [x + pad, y, (w - 2.0 * pad).max(1.0), h]
}

/// The glyph origin that centres `text` vertically in a box whose centre is
/// `center_y`.
///
/// [`queue_text_glyphs`] takes the **top of the first line**, not a baseline:
/// `y_offset` is measured down from there. Handing it the box's centre - which
/// every UI path did until Sep 2026 - therefore hung the text below the
/// centre, and out of the bottom of any box less than twice the font's line
/// height tall. That is the half of BUGS #60 that showed up in the GUI path
/// too: a 14px label in a 24px button sat on its lower edge.
pub(crate) fn centered_line_top(font: &FontData, text: &str, scale: f32, center_y: f32) -> f32 {
    // Multi-line text grows downward from the origin, so the block's own
    // height decides how far above the centre it starts.
    let lines = text.lines().count().max(1) as f32;
    center_y + font.line_height * scale * lines * 0.5
}

/// [`queue_text_glyphs`] with an optional wrap width, in the same scaled
/// units the glyphs are drawn in. `None` wraps only on explicit newlines.
/// `align` places each line about `position.x` by that line's own width.
#[allow(clippy::too_many_arguments)]
pub(crate) fn queue_text_glyphs_wrapped(
    font_data: &FontData,
    batch: &mut WgpuSpriteBatch,
    text: &str,
    position: Vec2,
    scale: f32,
    color: Vec4,
    wrap_width: Option<f32>,
    align: TextAlign,
) -> usize {
    queue_text_glyphs_rotated(font_data, batch, text, position, scale, 0.0, color, wrap_width, align)
}

/// [`queue_text_glyphs_wrapped`], turned by `rotation` radians about
/// `position`. World text takes its entity's `Transform.rotation` through
/// here; screen-space UI passes 0.
///
/// The placement itself lives in `text_layout::glyph_quads`, which is a pure
/// function and therefore testable without a GPU - this is only the part
/// that pushes quads into a batch.
#[allow(clippy::too_many_arguments)]
pub(crate) fn queue_text_glyphs_rotated(
    font_data: &FontData,
    batch: &mut WgpuSpriteBatch,
    text: &str,
    position: Vec2,
    scale: f32,
    rotation: f32,
    color: Vec4,
    wrap_width: Option<f32>,
    align: TextAlign,
) -> usize {
    let quads =
        crate::renderer::text_layout::glyph_quads(font_data, text, position, scale, rotation, wrap_width, align);
    for q in &quads {
        batch.draw_sprite_uv(q.center, q.size, q.rotation, color, q.uv);
    }
    quads.len()
}

/// A rich text's glyphs (`renderer::rich_text`), each in its character's
/// style: its colour, bold (drawn twice, a hair apart), the wave / shake /
/// rainbow effects at the batch's clock, and the typewriter - characters
/// past `visible.0` not drawn, the one being revealed fading in.
#[allow(clippy::too_many_arguments)]
fn queue_rich_text(
    batch: &mut WgpuSpriteBatch,
    font: &FontData,
    text: &TextComponent,
    rich: &crate::renderer::rich_text::RichText,
    visible: (usize, f32),
    position: Vec2,
    entity_scale: f32,
    rotation: f32,
    mul: Vec4,
    // A shadow's or outline's one colour, in place of the markup's.
    solid: Option<Vec4>,
) -> usize {
    let scale = text.font_scale * entity_scale;
    let wrap = (text.wrap_width > 0.0).then_some(text.wrap_width);
    let places = crate::renderer::text_layout::glyph_places(font, &rich.plain, position, scale, rotation, wrap, text.align);
    let line = font.line_height * scale;
    let (sin, cos) = rotation.sin_cos();
    let turn = |v: Vec2| Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos);
    let time = batch.time();
    let mut drawn = 0;
    for p in &places {
        if p.index > visible.0 || (p.index == visible.0 && visible.1 <= 0.0) {
            continue;
        }
        let style = rich.styles.get(p.index).copied().unwrap_or_default();
        let base = style.color.unwrap_or(text.color);
        let (offset, rainbow) = crate::renderer::rich_text::effect(&style, p.index, time, base);
        let rgb = rainbow.unwrap_or(base);
        let mut alpha = text.color[3] * style.color.map_or(1.0, |c| c[3]);
        if p.index == visible.0 {
            alpha *= visible.1;
        }
        let color = match solid {
            Some(c) => Vec4::new(c.x, c.y, c.z, c.w * alpha / text.color[3].max(1e-4)) * mul,
            None => Vec4::new(rgb[0], rgb[1], rgb[2], alpha) * mul,
        };
        let center = p.quad.center + turn(offset * line);
        batch.draw_sprite_uv(center, p.quad.size, p.quad.rotation, color, p.quad.uv);
        if style.bold {
            batch.draw_sprite_uv(center + turn(Vec2::new(line * 0.035, 0.0)), p.quad.size, p.quad.rotation, color, p.quad.uv);
        }
        drawn += 1;
    }
    drawn
}

/// Render every `Transform + Tilemap` entity, bucketed by tileset texture for
/// minimum draw calls.
///
/// Phase 3.c-4 Step 5b: wgpu twin of [`crate::ecs::systems::render_tilemaps`].
/// Mirrors the gl path's frustum-cull-then-bucket-by-tileset strategy:
/// 1. Computes the visible tile range from the camera bounds.
/// 2. Walks visible cells, resolves each gid → (tileset_idx, local_id).
/// 3. Buckets per tileset_idx so each tileset image gets one draw call.
/// 4. For each non-empty bucket, calls `batch.begin()`, queues all glyph
///    quads with their per-tile UVs, then invokes `flush_group(batch,
///    &tileset.texture_id)`. The caller resolves the matching wgpu texture
///    (typically via `AssetManager::ensure_wgpu_texture`) and calls
///    `batch.flush(...)` against it.
///
/// Returns the total number of tile quads queued across all buckets.
///
/// Caller is responsible for `begin_main_pass` / `present`. Skips
/// `collision: true` and `visible: false` layers exactly like the gl path.
/// The sort key below which a sprite is a **backdrop**: drawn before every
/// tilemap instead of after them. `None` when the world has no tilemap.
///
/// Tilemaps are their own pass, drawn before the sprite pass, so a sprite
/// always covered every tilemap whatever the z orders said - a parallax sky
/// hid a scrolling tile terrain entirely. A sprite sorted below the lowest
/// tilemap now draws beneath the tilemaps, which is what its z order meant.
pub fn backdrop_ceiling(world: &World, sort_layers: &SortLayers) -> Option<i32> {
    world
        .query::<&Tilemap>()
        .iter()
        .map(|(_, tm)| sort_layers.resolve(tm.sort_layer.as_deref(), tm.z_order))
        .min()
}

/// Whether a sprite with this sort key draws in the backdrop pass. Only the
/// default pipeline takes part: a material sprite or an `Unlit` one keeps
/// its own pass.
#[inline]
fn is_backdrop(key: i32, material: Option<&str>, unlit: bool, ceiling: Option<i32>) -> bool {
    !unlit && material.is_none() && ceiling.is_some_and(|c| key < c)
}

/// Draw the backdrop sprites (see [`backdrop_ceiling`]), back to front.
fn render_backdrop_sprites(
    world: &World,
    sort_layers: &SortLayers,
    batch: &mut WgpuSpriteBatch,
    flush_group: &mut impl FnMut(&mut WgpuSpriteBatch, &str),
) -> usize {
    let Some(ceiling) = backdrop_ceiling(world, sort_layers) else { return 0 };
    let view = frame_view();
    let fallback = Vec2::from(SpriteComponent::FALLBACK_SIZE);
    let mut query = world.query::<(
        &Transform,
        &SpriteComponent,
        Option<&WorldTransform>,
        Option<&MaterialComponent>,
        Option<&crate::ecs::components::Unlit>,
    )>();
    let mut drawables: Vec<(WorldTransform, &SpriteComponent, i32)> = query
        .iter()
        .filter_map(|(e, (t, s, w, m, unlit))| {
            let key = sort_layers.resolve(s.sort_layer.as_deref(), s.z_order);
            let material = m.map(|m| m.material_id.as_str());
            if !(is_backdrop(key, material, unlit.is_some(), Some(ceiling)) && s.color[3] > 0.0)
                || !in_z_window(key)
                || !in_view_layers(world, e)
            {
                return None;
            }
            let transform = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
            if let Some(v) = view
                && !v.sees_sprite(transform.position, transform.scale, s, fallback)
            {
                return None;
            }
            Some((transform, s, key))
        })
        .collect();
    drawables.sort_by(|a, b| {
        a.2.cmp(&b.2).then_with(|| {
            crate::asset::resolve_texture_id(&a.1.texture_id).cmp(crate::asset::resolve_texture_id(&b.1.texture_id))
        })
    });
    let default_size = Vec2::from(SpriteComponent::FALLBACK_SIZE);
    let mut total = 0;
    let mut i = 0;
    while i < drawables.len() {
        let tex = crate::asset::resolve_texture_id(&drawables[i].1.texture_id);
        batch.begin();
        while i < drawables.len() && crate::asset::resolve_texture_id(&drawables[i].1.texture_id) == tex {
            queue_sprite(batch, &drawables[i].0, drawables[i].1, default_size);
            total += 1;
            i += 1;
        }
        flush_group(batch, tex);
    }
    total
}

/// One tile of a tilemap layer, placed in the world: what the tilemap pass
/// draws, and what the sprite pass sorts for a `y_sort` layer.
struct PlacedTile {
    tileset: usize,
    local: u32,
    center: Vec2,
    size: Vec2,
    /// The height it sorts by - on the grid, whatever the layer's `offset`.
    foot_y: f32,
}

/// Every tile of `layer` that may show in the box `center +- half`, row by
/// row (painter's order for an isometric map): an autotiled layer's dual
/// cells (`tilemap::autotile`), each a tile per terrain reaching it, bottom
/// terrain first - or a plain layer's cells. `origin` is the entity's
/// position. The shape of the grid is [`Tilemap::grid_to_local`]'s.
fn place_layer_tiles(
    tilemap: &Tilemap,
    origin: Vec2,
    layer: &crate::tilemap::TilemapLayer,
    rule: Option<&crate::tilemap::autotile::AutotileLayer>,
    center: Vec2,
    half: Vec2,
    out: &mut impl FnMut(PlacedTile),
) {
    let [tw, th] = tilemap.tile_size;
    if tw <= 0.0 || th <= 0.0 {
        return;
    }
    let base = origin + Vec2::from(layer.offset);
    let lo = center - half - base;
    let hi = center + half - base;
    let (cols, rows) = tilemap.cells_in_box(layer.width, layer.height, lo.into(), hi.into());
    // A lifted layer sorts by its cells, a hair in front of whatever is on
    // the same cell below it - a house on a block draws after the block,
    // and still behind anything a cell further forward.
    let ground = origin.y - layer.offset[1] / 64.0;
    // A dual grid is square corners; a hexagonal map draws its cells as
    // they are, terrain numbers and all.
    if let Some(rule) = rule.filter(|_| !tilemap.is_hex()) {
        // A dual cell sits on a corner, so there is one more each way.
        for dy in rows.start..=rows.end {
            for dx in cols.start..=cols.end {
                let (x, y) = tilemap.grid_to_local(dx as f32, dy as f32);
                for gid in crate::tilemap::autotile::dual_cell_gids(layer, rule, dx as i64, dy as i64) {
                    if let Some((tileset, local)) = tilemap.resolve_gid(gid) {
                        let local = tilemap.tilesets[tileset].drawn(local, tilemap.anim_time, dx as i64, dy as i64);
                        let size = Vec2::new(tw, th);
                        out(PlacedTile { tileset, local, center: base + Vec2::new(x, y), size, foot_y: ground + y });
                    }
                }
            }
        }
        return;
    }
    for row in rows {
        for col in cols.clone() {
            let Some(gid) = layer.get_tile(col, row) else { continue };
            let Some((tileset, local)) = tilemap.resolve_gid(gid) else { continue };
            let local = tilemap.tilesets[tileset].drawn(local, tilemap.anim_time, col as i64, row as i64);
            let (c, size, foot) = tilemap.cell_quad(&tilemap.tilesets[tileset], col, row);
            out(PlacedTile { tileset, local, center: base + Vec2::from(c), size: Vec2::from(size), foot_y: ground + foot });
        }
    }
}

pub fn render_tilemaps_wgpu(
    world: &World,
    sort_layers: &SortLayers,
    batch: &mut WgpuSpriteBatch,
    camera: &Camera2D,
    mut flush_group: impl FnMut(&mut WgpuSpriteBatch, &str),
) -> usize {
    let cam_pos = camera.position;
    // What the camera can see, as half-extents: a turned camera sees a
    // wider box than its viewport, because the corners swing out.
    let cam_half = camera.half_extents();

    // Sprites sorted below every tilemap go first: a parallax sky behind a
    // scrolling tile terrain. The sprite passes skip the same ones.
    let mut total = render_backdrop_sprites(world, sort_layers, batch, &mut flush_group);

    // Iterate tilemap entities in resolved-z order (intra-pass only —
    // see Tilemap.sort_layer doc; this controls tilemap-vs-tilemap
    // ordering, not interleaving with sprites).
    let mut tilemap_ents: Vec<(hecs::Entity, i32)> = world
        .query::<&Tilemap>()
        .iter()
        .map(|(e, tm)| (e, sort_layers.resolve(tm.sort_layer.as_deref(), tm.z_order)))
        .filter(|(e, z)| in_z_window(*z) && in_view_layers(world, *e))
        .collect();
    tilemap_ents.sort_by_key(|(_, z)| *z);

    for (entity, _) in tilemap_ents {
        let Ok(transform) = world.get::<&Transform>(entity) else { continue };
        let Ok(tilemap) = world.get::<&Tilemap>(entity) else { continue };
        // Borrowed, not cloned: a long scrolling stage is thousands of tiles.
        let tilemap = &*tilemap;
        let origin = crate::tilemap::world_origin(world, entity, &transform);
        let tw = tilemap.tile_size[0];
        let th = tilemap.tile_size[1];
        if tw <= 0.0 || th <= 0.0 {
            continue;
        }

        let autotile = world.get::<&crate::tilemap::autotile::Autotile>(entity).ok();
        for layer in &tilemap.layers {
            // A `y_sort` layer is drawn by the sprite pass, sorted with sprites.
            if !layer.visible || layer.collision || layer.y_sort {
                continue;
            }
            let rule = autotile.as_ref().and_then(|a| a.for_layer(&layer.name));

            // Bucket by tileset index, each bucket in painter's order.
            let mut buckets: std::collections::HashMap<usize, Vec<(u32, Vec2, Vec2)>> =
                std::collections::HashMap::new();
            place_layer_tiles(tilemap, origin, layer, rule, cam_pos, cam_half, &mut |t| {
                buckets.entry(t.tileset).or_default().push((t.local, t.center, t.size));
            });

            // One flush per tileset texture, in tileset order - so an
            // autotiled layer whose terrains share one tileset stacks them.
            let mut buckets: Vec<(usize, Vec<(u32, Vec2, Vec2)>)> = buckets.into_iter().collect();
            buckets.sort_by_key(|(i, _)| *i);
            for (ts_idx, tiles) in &buckets {
                let ts = &tilemap.tilesets[*ts_idx];
                batch.begin();
                let mut queued = 0usize;
                for &(local_id, pos, size) in tiles {
                    let Some(uv) = ts.tile_uv(local_id) else {
                        continue;
                    };
                    // A layer can show more tiles of one tileset than a batch
                    // holds (8192): flush and carry on, as the particle pass
                    // does. It used to queue them all and flush once, so the
                    // batch dropped every tile past the limit - a territory-claiming
                    // game lost the bottom rows of a 160 x 96 picture.
                    if batch.is_full() {
                        flush_group(batch, &ts.texture_id);
                        total += queued;
                        queued = 0;
                        batch.begin();
                    }
                    batch.draw_sprite_uv(pos, size, 0.0, Vec4::ONE, uv);
                    queued += 1;
                }
                if queued > 0 {
                    flush_group(batch, &ts.texture_id);
                    total += queued;
                }
            }
        }
    }

    total
}

/// Render every alive particle from `particle_system`, bucketed by emitter
/// texture for one draw call per texture (with auto-flush mid-stream when
/// the batch fills up).
///
/// Phase 3.c-4 Step 5b/5c: wgpu twin of [`crate::ecs::systems::render_particles`].
/// Walks `Transform + ParticleEmitter` to gather emitter world positions,
/// then asks `ParticleSystem::draw_data` to resolve local-vs-world-space
/// particles into absolute draw entries. The returned groups carry an
/// `Option<String>` texture id — `None` means "no texture, draw white quads"
/// (caller passes `None` to `flush_group`'s second arg).
///
/// Caller responsibilities:
/// - Provide a `flush_group(batch, Option<&str>)` callback that resolves the
///   wgpu texture (typically via `AssetManager::ensure_wgpu_texture`, falling
///   back to a 1×1 white texture for `None`) and calls `batch.flush(...)`.
/// - Drive `begin_main_pass` / `present` around the call.
///
/// Returns the total number of particles queued across all groups.
///
/// **Limitation vs gl path:** the gl renderer applies per-emitter blend
/// modes (additive/alpha) before each flush. The wgpu sprite pipeline is
/// fixed to alpha-blend in this slice — additive particles will look wrong
/// until Step 5e/Phase 4 lands the pipeline-cache + per-blend-mode
/// pipelines. Callers needing additive can swap to a custom batch in the
/// meantime.
pub fn render_particles_wgpu(
    world: &World,
    sort_layers: &SortLayers,
    particle_system: &ParticleSystem,
    batch: &mut WgpuSpriteBatch,
    flush_group: impl FnMut(&mut WgpuSpriteBatch, Option<&str>),
) -> usize {
    render_particles_in_layer(world, sort_layers, particle_system, batch, WorldLayer::All, flush_group)
}

/// [`render_particles_wgpu`] for one layer of a lit frame ([`WorldLayer`]):
/// an emitter whose entity is [`Unlit`](crate::ecs::components::Unlit) is a
/// cutout in the scene layer and draws itself in the emission layer, and a lit
/// one the other way round - the rule sprites follow ([`layer_draw`]).
///
/// Particles used to draw only in the scene layer, so `unlit` on an emitter
/// did nothing: a snowstorm on a moonlit night came out as dim as the night,
/// and only an additive emitter ever looked unlit, by luck of its glow. In a
/// lit frame lit and unlit emitters draw as two sets, lit first; a frame with
/// no lighting (`All`) draws as it always did.
pub fn render_particles_in_layer(
    world: &World,
    sort_layers: &SortLayers,
    particle_system: &ParticleSystem,
    batch: &mut WgpuSpriteBatch,
    layer: WorldLayer,
    mut flush_group: impl FnMut(&mut WgpuSpriteBatch, Option<&str>),
) -> usize {
    // Match gl path: collect (entity_id, &emitter, world_pos) tuples for
    // `draw_data` so local-space particles get resolved correctly.
    // At the world position `tick_particles` simulated them at.
    let mut query = world.query::<(
        &Transform,
        &ParticleEmitter,
        Option<&WorldTransform>,
        Option<&crate::ecs::components::Unlit>,
    )>();
    let emitters: Vec<(u64, &ParticleEmitter, Vec2, bool)> = query
        .iter()
        .filter(|(e, (_, emitter, _, _))| {
            in_z_window(sort_layers.resolve(emitter.sort_layer.as_deref(), emitter.z_order)) && in_view_layers(world, *e)
        })
        .map(|(entity, (transform, emitter, wt, unlit))| {
            (entity.id() as u64, emitter, wt.map_or(transform.position, |w| w.position), unlit.is_some())
        })
        .collect();

    // A masked emitter's particles are clipped like a sprite (`renderer::mask`).
    let mut masks = MaskTable::gather(world);
    let mask_of: std::collections::HashMap<u64, u32> = world
        .query::<(&ParticleEmitter, &crate::ecs::components::Masked)>()
        .iter()
        .map(|(e, (_, m))| (e.id() as u64, masks.index(Some(m))))
        .filter(|(_, i)| *i != 0)
        .collect();
    // Without lighting everything is one set, drawn as itself.
    let sets: &[Option<bool>] = if layer == WorldLayer::All { &[None] } else { &[Some(false), Some(true)] };
    let mut total = 0usize;

    for &set in sets {
        let chosen: Vec<(u64, &ParticleEmitter, Vec2)> = emitters
            .iter()
            .filter(|e| set.is_none_or(|unlit| e.3 == unlit))
            .map(|e| (e.0, e.1, e.2))
            .collect();
        if chosen.is_empty() {
            continue;
        }
        let groups = particle_system.draw_data_masked(&chosen, sort_layers, &mask_of);
        for group in &groups {
            let particles = &group.particles;
            if particles.is_empty() {
                continue;
            }
            let Some((cutout, blend)) = layer_draw(layer, set.unwrap_or(false), group.blend) else { continue };
            let tint = if cutout { CUTOUT } else { Vec4::ONE };

            let tex_key: Option<&str> = group.texture.as_deref();
            batch.begin();
            // The emitter's blend (`ParticleEmitter::blend_mode`) - additive for
            // fire and sparks. Every particle used to draw alpha-blended.
            batch.set_blend(blend);
            masks.apply(group.mask, batch, &mut |b, t| flush_group(b, Some(t)));
            let mut queued = 0usize;

            // Trails under their particles, a quad a segment (`ParticleTrail`).
            for trail in &group.trails {
                if batch.is_full() {
                    flush_group(batch, tex_key);
                    total += queued;
                    queued = 0;
                    batch.begin();
                }
                let (pos, uvq, mut col, idx) = trail.ribbon();
                if cutout {
                    for c in &mut col {
                        *c = (Vec4::from(*c) * tint).to_array();
                    }
                }
                queued += batch.draw_mesh(&pos, &uvq, &col, &idx);
            }

            for p in particles {
                // Auto-flush mid-stream when the batch fills, mirroring the gl
                // path's `is_full() → flush → continue queuing` pattern.
                if batch.is_full() {
                    flush_group(batch, tex_key);
                    total += queued;
                    queued = 0;
                    batch.begin();
                }
                batch.draw_sprite_uv(
                    p.position,
                    Vec2::new(p.length, p.size),
                    p.rotation,
                    Vec4::from(p.color) * tint,
                    p.uv,
                );
                queued += 1;
            }

            if queued > 0 {
                flush_group(batch, tex_key);
                total += queued;
            }
        }
    }
    batch.set_blend(crate::renderer::material::BlendMode::Alpha);
    batch.set_mask(crate::renderer::mask::MaskUniform::NONE);

    total
}

/// Render every entity with a `UIElement` component in screen space.
///
/// Iterates `UIElement`s in `z_order` and draws each entity's widget: panel,
/// image, text, button, progress bar, slider, toggle and text input. Quads
/// and glyphs need different textures and the batch is single-texture, so
/// every piece is flushed separately, in order.
///
/// **Only panels, images and text used to be drawn.** The other five widget
/// components had scene serialization, `set_ui_prop` / `get_ui_prop` and an
/// inspector, and rendered nothing in any runtime - a progress bar driven
/// from script was invisible, and a `UIButton` without a sibling `UIPanel`
/// had hover and press state but no pixels.
///
/// Caller responsibilities (single callback invoked once per element-piece):
/// - `flush(batch, UiFlush::Quad(None))`    ⇒ bind a 1×1 white texture (panels)
/// - `flush(batch, UiFlush::Quad(Some(id)))` ⇒ resolve user texture (images)
/// - `flush(batch, UiFlush::Font(id))`       ⇒ bind that font's atlas
///
/// Merged into a single callback (was two as of 2026-05-05 to make wedge 4
/// in `wgpu_scene_runner` borrow-checker friendly: two closures both
/// capturing `&mut AssetManager` + `&mut RenderPass<'_>` cannot coexist).
///
/// Note: this fn does *not* construct the screen-space orthographic VP.
/// Callers must build it (`Mat4::orthographic_rh_gl(0, screen_w, 0,
/// screen_h, -1, 1)`) and use it when calling `batch.flush(...)` inside the
/// callback, so the VP can be reused across `render_ui_wgpu` /
/// `render_gui_wgpu` / etc.
///
/// Text-bearing widgets are skipped when `fonts` is `None`.
///
/// Returns `(quads, images, glyphs)` counts; `quads` covers every untextured
/// widget piece, not only panels.
pub fn render_ui_wgpu(
    world: &World,
    batch: &mut WgpuSpriteBatch,
    fonts: Option<&FrameFonts<'_>>,
    screen_w: f32,
    screen_h: f32,
    mut flush: impl FnMut(&mut WgpuSpriteBatch, UiFlush<'_>),
) -> (usize, usize, usize) {
    use crate::ui::{UIProgressBar, UISlider, UITextInput, UIToggle};

    // Collect + sort by z_order. Entities are kept whole: the per-widget
    // lookups below go straight to them. They used to go through a helper
    // that scanned every entity carrying the component for a matching id -
    // up to four scans per element per frame.
    let mut drawables: Vec<(i32, hecs::Entity, UIElement)> = Vec::new();
    for (entity, ui_elem) in world.query::<&UIElement>().iter() {
        if !ui_elem.visible {
            continue;
        }
        drawables.push((ui_elem.z_order, entity, ui_elem.clone()));
    }
    drawables.sort_by_key(|(z, _, _)| *z);

    let mut quads = 0usize;
    let mut images = 0usize;
    let mut glyphs = 0usize;

    for (_, entity, ui_elem) in &drawables {
        let Ok(e) = world.entity(*entity) else { continue };
        let center = ui_elem.screen_center(screen_w, screen_h);
        let size = ui_elem.size;
        let left = center.x - size.x * 0.5;
        let button_color = e.get::<&UIButton>().map(|b| button_display_color(&b));

        // Panel background; a button on the same entity supplies the colour.
        if let Some(panel) = e.get::<&UIPanel>() {
            let color = button_color.unwrap_or(panel.bg_color);
            batch.begin();
            batch.draw_sprite_uv(center, size, 0.0, Vec4::from(color), FULL_UV);
            flush(batch, UiFlush::Quad(None));
            quads += 1;
        } else if let Some(color) = button_color {
            // A button with no panel is still a button.
            batch.begin();
            batch.draw_sprite_uv(center, size, 0.0, Vec4::from(color), FULL_UV);
            flush(batch, UiFlush::Quad(None));
            quads += 1;
        }

        // Controller focus (`navigate_ui`): the `.gui` focus ring's colour,
        // just outside the element so nothing drawn on it covers the ring.
        if ui_elem.focused {
            batch.begin();
            quads += draw_outline(batch, center, size + Vec2::splat(6.0), 2.0, [0.54, 0.71, 0.98, 0.9]);
            flush(batch, UiFlush::Quad(None));
        }

        // Textured image quad.
        if let Some(ui_img) = e.get::<&UIImage>() {
            let uv = ui_img.uv_rect.unwrap_or(FULL_UV);
            let color = Vec4::from(ui_img.color);
            batch.begin();
            match (ui_img.nine_slice, ui_img.source_px) {
                // The sprites' own slicing, with the borders and the source
                // both scaled: drawn `scale` times as wide, sampling the
                // same texture pixels.
                (Some(n), Some(src)) => {
                    let k = n.scale.max(0.01);
                    emit_nine_slice(
                        size,
                        uv,
                        [0.5, 0.5],
                        [n.left * k, n.right * k, n.top * k, n.bottom * k],
                        Some([src[0] * k, src[1] * k]),
                        &mut |sub, sub_uv, anchor| batch.draw_sprite_anchored(center, sub, 0.0, color, sub_uv, anchor),
                    );
                }
                _ => batch.draw_sprite_uv(center, size, 0.0, color, uv),
            }
            flush(batch, UiFlush::image(&ui_img.texture_id));
            images += 1;
        }

        // Progress bar: background, then the fill growing from the left edge.
        if let Some(bar) = e.get::<&UIProgressBar>() {
            let t = fraction(bar.value, bar.min, bar.max);
            batch.begin();
            batch.draw_sprite_uv(center, size, 0.0, Vec4::from(bar.bg_color), FULL_UV);
            quads += 1;
            if t > 0.0 {
                let w = size.x * t;
                batch.draw_sprite_uv(
                    Vec2::new(left + w * 0.5, center.y),
                    Vec2::new(w, size.y),
                    0.0,
                    Vec4::from(bar.fg_color),
                    FULL_UV,
                );
                quads += 1;
            }
            if bar.border_color[3] > 0.0 {
                quads += draw_outline(batch, center, size, 1.0, bar.border_color);
            }
            flush(batch, UiFlush::Quad(None));
        }

        // Slider: a thin track across the full width, and a square thumb at
        // the value.
        if let Some(slider) = e.get::<&UISlider>() {
            let t = fraction(slider.value, slider.min, slider.max);
            let track_h = (size.y * 0.25).max(2.0);
            let thumb = slider.thumb_size.max(1.0);
            batch.begin();
            batch.draw_sprite_uv(
                center,
                Vec2::new(size.x, track_h),
                0.0,
                Vec4::from(slider.track_color),
                FULL_UV,
            );
            batch.draw_sprite_uv(
                Vec2::new(left + size.x * t, center.y),
                Vec2::splat(thumb),
                0.0,
                Vec4::from(slider.thumb_color),
                FULL_UV,
            );
            flush(batch, UiFlush::Quad(None));
            quads += 2;
        }

        // Toggle: a track in the on / off colour, and a thumb at the matching
        // end.
        if let Some(toggle) = e.get::<&UIToggle>() {
            let track = if toggle.checked { toggle.on_color } else { toggle.off_color };
            let thumb = (size.y * 0.8).min(size.x * 0.5).max(1.0);
            let inset = (size.y - thumb) * 0.5;
            let thumb_x = if toggle.checked {
                left + size.x - inset - thumb * 0.5
            } else {
                left + inset + thumb * 0.5
            };
            batch.begin();
            batch.draw_sprite_uv(center, size, 0.0, Vec4::from(track), FULL_UV);
            batch.draw_sprite_uv(
                Vec2::new(thumb_x, center.y),
                Vec2::splat(thumb),
                0.0,
                Vec4::from(toggle.thumb_color),
                FULL_UV,
            );
            flush(batch, UiFlush::Quad(None));
            quads += 2;
        }

        // Text input: background, a focus outline, then the text - or the
        // placeholder, dimmed, when empty.
        if let Some(input) = e.get::<&UITextInput>() {
            batch.begin();
            batch.draw_sprite_uv(center, size, 0.0, Vec4::from(input.bg_color), FULL_UV);
            quads += 1;
            if input.focused {
                quads += draw_outline(batch, center, size, 1.0, input.color);
            }
            flush(batch, UiFlush::Quad(None));

            // The caret, the selection, a long line scrolled to keep the
            // caret in sight (`ui::text_edit`).
            if let Some(fonts) = fonts {
                let (fd, font_tex) = fonts.resolve(&input.font_id);
                let scale = input.font_size / fd.pixel_size;
                let bottom = center.y - size.y * 0.5;
                let rect = [left, bottom, size.x, size.y];
                let plan = plan_text_field(
                    fd, &input.text, &input.placeholder, input.password, &input.edit, input.focused,
                    crate::ui::text_edit::entity_key(*entity), rect, rect, scale, Vec4::from(input.color),
                );
                let c = input.color;
                if let Some((at, sz)) = plan.selection {
                    batch.begin();
                    batch.draw_sprite_uv(at, sz, 0.0, Vec4::new(c[0], c[1], c[2], c[3] * 0.3), FULL_UV);
                    flush(batch, UiFlush::Quad(None));
                    quads += 1;
                }
                if !plan.shown.is_empty() {
                    batch.begin();
                    glyphs += queue_text_glyphs(fd, batch, &plan.shown, plan.text_pos, scale, plan.color);
                    flush(batch, UiFlush::Font(font_tex));
                }
                for (at, sz) in plan.caret.into_iter().chain(plan.underline) {
                    batch.begin();
                    batch.draw_sprite_uv(at, sz, 0.0, Vec4::from(c), FULL_UV);
                    flush(batch, UiFlush::Quad(None));
                    quads += 1;
                }
            }
        }

        // Text glyphs.
        if let Some(ui_text) = e.get::<&UIText>()
            && let Some(fonts) = fonts
        {
            let piece = UiTextPiece {
                text: &ui_text.text,
                font_id: &ui_text.font_id,
                font_size: ui_text.font_size,
                color: ui_text.color,
                alignment: ui_text.alignment,
                outline: ui_text.outline,
                shadow: ui_text.shadow,
                rich: ui_text.rich,
                reveal_speed: ui_text.reveal_speed,
                reveal: ui_text.reveal.map(|r| r.elapsed),
            };
            glyphs += draw_ui_text(fonts, batch, &mut flush, &piece, center, size);
        }
    }

    (quads, images, glyphs)
}

const FULL_UV: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

/// `value` as a 0..1 position between `min` and `max`.
fn fraction(value: f32, min: f32, max: f32) -> f32 {
    let span = max - min;
    if span.abs() <= f32::EPSILON {
        return 0.0;
    }
    ((value - min) / span).clamp(0.0, 1.0)
}

/// Half brightness **as seen**, alpha kept: what "disabled" looks like.
///
/// UI colours are linear, so the half is taken in sRGB and converted back -
/// halving the linear value reads as about 73% (a 0.95 label drew at 0.70),
/// and a disabled button looked enabled (Oct 2026).
fn dimmed(c: [f32; 4]) -> [f32; 4] {
    fn half(v: f32) -> f32 {
        let v = v.max(0.0);
        let s = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 } * 0.5;
        if s <= 0.040_45 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    }
    [half(c[0]), half(c[1]), half(c[2]), c[3]]
}

/// A `UIButton`'s current display colour (hover / pressed / disabled).
fn button_display_color(btn: &UIButton) -> [f32; 4] {
    if btn.disabled {
        dimmed(btn.normal_color)
    } else if btn.pressed {
        btn.pressed_color
    } else if btn.hovered {
        btn.hover_color
    } else {
        btn.normal_color
    }
}

/// Queue a `thickness`-wide rectangle outline. Returns the quads queued.
fn draw_outline(
    batch: &mut WgpuSpriteBatch,
    center: Vec2,
    size: Vec2,
    thickness: f32,
    color: [f32; 4],
) -> usize {
    let c = Vec4::from(color);
    let half = size * 0.5;
    let t = thickness;
    batch.draw_sprite_uv(center + Vec2::new(0.0, half.y - t * 0.5), Vec2::new(size.x, t), 0.0, c, FULL_UV);
    batch.draw_sprite_uv(center - Vec2::new(0.0, half.y - t * 0.5), Vec2::new(size.x, t), 0.0, c, FULL_UV);
    batch.draw_sprite_uv(center - Vec2::new(half.x - t * 0.5, 0.0), Vec2::new(t, size.y), 0.0, c, FULL_UV);
    batch.draw_sprite_uv(center + Vec2::new(half.x - t * 0.5, 0.0), Vec2::new(t, size.y), 0.0, c, FULL_UV);
    4
}

/// One run of UI text, for [`draw_ui_text`].
struct UiTextPiece<'a> {
    text: &'a str,
    font_id: &'a str,
    font_size: f32,
    color: [f32; 4],
    alignment: TextAlign,
    outline: Option<crate::ecs::components::TextOutline>,
    shadow: Option<crate::ecs::components::TextShadow>,
    /// Markup (`renderer::rich_text`), and the typewriter: its speed and its
    /// clock, as world text has them.
    rich: bool,
    reveal_speed: f32,
    reveal: Option<f32>,
}

/// Where each inline icon of `rich` goes: its texture id's byte range in
/// the markup and the centre of the spaces it stands on - the icons the
/// typewriter has reached (`visible`), laid out as `glyph_places` lays
/// out the plain text.
#[allow(clippy::too_many_arguments)]
fn rich_icon_spots(
    font: &FontData,
    rich: &crate::renderer::rich_text::RichText,
    visible: (usize, f32),
    position: Vec2,
    scale: f32,
    rotation: f32,
    wrap: Option<f32>,
    align: TextAlign,
) -> Vec<(std::ops::Range<usize>, Vec2)> {
    if rich.icons.is_empty() {
        return Vec::new();
    }
    let places = crate::renderer::text_layout::glyph_places(font, &rich.plain, position, scale, rotation, wrap, align);
    rich.icons
        .iter()
        .filter(|i| i.at < visible.0)
        .filter_map(|icon| {
            let mut cells = places.iter().filter(|p| p.index >= icon.at && p.index < icon.at + icon.len);
            let first = cells.next()?.cell_center;
            let last = cells.last().map_or(first, |p| p.cell_center);
            Some((icon.id.clone(), (first + last) * 0.5))
        })
        .collect()
}

/// Rich UI text's glyphs - [`queue_rich_text`], flat and unscaled by an
/// entity, `at` the top-left of the first line.
#[allow(clippy::too_many_arguments)]
fn queue_rich_ui(
    font: &FontData,
    batch: &mut WgpuSpriteBatch,
    rich: &crate::renderer::rich_text::RichText,
    visible: (usize, f32),
    at: Vec2,
    scale: f32,
    color: [f32; 4],
    solid: Option<Vec4>,
    align: TextAlign,
) -> usize {
    let text = TextComponent { font_scale: scale, color, align, ..TextComponent::default() };
    queue_rich_text(batch, font, &text, rich, visible, at, 1.0, 0.0, Vec4::ONE, solid)
}

/// Draw `piece` aligned inside a box of `size` centred on `center`, and flush
/// it with its font. Returns the glyph count.
fn draw_ui_text(
    fonts: &FrameFonts<'_>,
    batch: &mut WgpuSpriteBatch,
    flush: &mut impl FnMut(&mut WgpuSpriteBatch, UiFlush<'_>),
    piece: &UiTextPiece<'_>,
    center: Vec2,
    size: Vec2,
) -> usize {
    if piece.text.is_empty() {
        return 0;
    }
    let (fd, font_tex) = fonts.resolve(piece.font_id);
    let scale = piece.font_size / fd.pixel_size;
    let rich = if piece.rich {
        Some(crate::renderer::rich_text::parse(piece.text, crate::renderer::rich_text::icon_spaces(fd)))
    } else if crate::renderer::rich_text::reveal_running(false, piece.reveal_speed, piece.reveal) {
        Some(crate::renderer::rich_text::literal(piece.text))
    } else {
        None
    };
    let visible = rich.as_ref().map_or((usize::MAX, 0.0), |rt| {
        crate::renderer::rich_text::visible_chars(rt, piece.reveal_speed, piece.reveal)
    });
    // Markup is measured as the text it lays out.
    let laid = rich.as_ref().map_or(piece.text, |rt| rt.plain.as_str());
    // Each line is placed about this x by its own width, as world text's
    // are (`text_layout`). Measuring the whole string as one line - which
    // this did until Oct 2026 - centred a two-line caption by the length of
    // both lines laid end to end, so it hung far off to the left.
    let text_x = match piece.alignment {
        TextAlign::Left => center.x - size.x * 0.5,
        TextAlign::Center => center.x,
        TextAlign::Right => center.x + size.x * 0.5,
    };
    let at = Vec2::new(text_x, centered_line_top(fd, laid, scale, center.y));
    // The shadow, the outline, then the text - each a flush of its own, its
    // edge in the params as world text's are (`TextStyle`), put back after.
    let mut layers: Vec<(Vec4, Vec2, TextStyle)> = Vec::with_capacity(3);
    if let Some(sh) = piece.shadow {
        let style = if fd.sdf {
            let soften = field_units(fd, scale, sh.softness);
            let grow = piece.outline.map_or(0.0, |o| field_units(fd, scale, o.width)) + soften;
            TextStyle::Field { grow: grow.min(0.45), soften }
        } else {
            piece.outline.map_or(TextStyle::Plain, |o| TextStyle::Copies { width: o.width })
        };
        // Faded with the text: a line faded out leaves no shadow behind.
        layers.push((faded(sh.color, piece.color[3]), Vec2::from(sh.offset), style));
    }
    if let Some(o) = piece.outline.filter(|o| o.width > 0.0) {
        let style = if fd.sdf {
            TextStyle::Field { grow: field_units(fd, scale, o.width), soften: 0.0 }
        } else {
            TextStyle::Copies { width: o.width }
        };
        layers.push((faded(o.color, piece.color[3]), Vec2::ZERO, style));
    }
    let styled = !layers.is_empty();
    let top = layers.len();
    layers.push((Vec4::from(piece.color), Vec2::ZERO, TextStyle::Plain));
    let mut n = 0;
    for (k, (color, nudge, style)) in layers.into_iter().enumerate() {
        batch.begin();
        batch.set_params(crate::renderer::material_params::from_bits(&style.params()));
        let spots: &[Vec2] = match style {
            TextStyle::Copies { .. } => &[
                Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(0.0, -1.0),
                Vec2::new(0.7, 0.7), Vec2::new(-0.7, 0.7), Vec2::new(0.7, -0.7), Vec2::new(-0.7, -0.7),
            ],
            _ => &[Vec2::ZERO],
        };
        let width = if let TextStyle::Copies { width } = style { width } else { 0.0 };
        for spot in spots {
            let p = at + nudge + *spot * width;
            n = match &rich {
                // A shadow or an outline is the markup's glyphs in one colour.
                Some(rt) => {
                    queue_rich_ui(fd, batch, rt, visible, p, scale, piece.color, (k < top).then_some(color), piece.alignment)
                }
                None => queue_text_glyphs_wrapped(fd, batch, piece.text, p, scale, color, None, piece.alignment),
            };
        }
        flush(batch, UiFlush::Font(font_tex));
    }
    if styled {
        batch.set_params([[0.0; 4]; 4]);
    }
    // Inline icons, each a square a line tall over its spaces.
    if let Some(rt) = &rich {
        let side = fd.line_height * scale * 0.9;
        for (id, spot) in rich_icon_spots(fd, rt, visible, at, scale, 0.0, None, piece.alignment) {
            batch.begin();
            batch.draw_sprite_uv(spot, Vec2::splat(side), 0.0, Vec4::new(1.0, 1.0, 1.0, piece.color[3]), FULL_UV);
            flush(batch, UiFlush::Quad(Some(&piece.text[id])));
        }
    }
    n
}

/// Draw on-screen touch controls - what
/// [`crate::input::touch_controls::TouchControlsState::draw_list`] returned -
/// in the screen-space UI pass, over everything else. Every host calls this
/// with the same `flush` it gives [`render_ui_wgpu`].
///
/// The draws are placed in the safe part of the screen, in virtual pixels
/// with y **down** - controls belong at the screen's edges, whatever the UI's
/// scale mode does, and clear of a notch. The UI projection is over the
/// *layout* area, y up and `layout_h` tall; `offset` is how far that area
/// sits from the safe area's corner (`UiScale::controls_offset`), and each
/// draw is moved into layout space by it here.
pub fn render_touch_controls_wgpu(
    draws: &[crate::input::touch_controls::TouchDraw],
    batch: &mut WgpuSpriteBatch,
    fonts: Option<&FrameFonts<'_>>,
    offset: Vec2,
    layout_h: f32,
    mut flush: impl FnMut(&mut WgpuSpriteBatch, UiFlush<'_>),
) {
    use crate::input::touch_controls::{DISC_TEXTURE_ID, RING_TEXTURE_ID};
    for d in draws {
        let center = Vec2::new(d.center.x - offset.x, layout_h - (d.center.y - offset.y));
        let size = Vec2::splat(d.radius * 2.0);
        batch.begin();
        batch.draw_sprite_uv(center, size, 0.0, Vec4::from(d.color), FULL_UV);
        flush(
            batch,
            UiFlush::Quad(Some(if d.ring { RING_TEXTURE_ID } else { DISC_TEXTURE_ID })),
        );
        if let (Some(label), Some(fonts)) = (d.label.as_deref(), fonts) {
            let piece = UiTextPiece {
                text: label,
                font_id: "",
                font_size: (d.radius * 0.7).max(10.0),
                color: [0.1, 0.1, 0.12, (d.color[3] * 1.6).min(1.0)],
                alignment: TextAlign::Center,
                outline: None,
                shadow: None,
                rich: false,
                reveal_speed: 0.0,
                reveal: None,
            };
            draw_ui_text(fonts, batch, &mut flush, &piece, center, size);
        }
    }
}

/// Render every visible [`GuiComponent`] document via the layout engine
/// + widget enum dispatch.
///
/// Phase 3.c-4 7-prep wedge 5 (2026-05-06): wgpu twin of
/// [`crate::ecs::systems::render_gui`]. Reuses the backend-agnostic
/// [`compute_layout`] + [`GuiDocument`] machinery; only the actual quad /
/// glyph emission is rewritten against [`WgpuSpriteBatch`] + the shared
/// [`UiFlush`] callback shape introduced by `render_ui_wgpu`.
///
/// Caller responsibilities (single callback, mirroring `render_ui_wgpu`):
/// - `flush(batch, UiFlush::Quad(None))`     ⇒ panels / borders / focus / widget chrome (white fallback)
/// - `flush(batch, UiFlush::Quad(Some(id)))` ⇒ image widget (caller resolves via `AssetManager`)
/// - `flush(batch, UiFlush::Font(id))`        ⇒ text / button labels / textinput / progressbar text
///
/// `fonts` is `Option<&FrameFonts>`; text-bearing widgets are silently
/// skipped when `None` (matches gl `Option<&BitmapFont>`). Each text-bearing
/// node resolves its own `font_id` through it, so one document can mix faces.
///
/// **API note (borrow-checker friendly):** unlike the gl path, this fn
/// does NOT take `&AssetManager`. The caller's `flush` closure typically
/// captures `&mut AssetManager` (for lazy `ensure_wgpu_texture` upload),
/// and a second `&AssetManager` arg here would conflict. Caller pre-walks
/// the world for visible `GuiComponent`s + resolves their `GuiDocument`s
/// via `AssetManager::get_gui` BEFORE constructing the closure, then
/// hands the resolved `(GuiDocument, GuiComponent)` pairs in here as
/// owned clones (typical scenes have 0–3 GUIs so cloning cost is
/// negligible).
///
/// Screen coordinate system: `(0, 0)` = top-left, Y down — matches the gl
/// path. Caller must build the matching `Mat4::orthographic_rh_gl(0,
/// screen_w, screen_h, 0, -1, 1)` and use it inside `flush` when calling
/// `batch.flush(...)`.
///
/// Returns `(quads, glyphs)` — total quad batches flushed and total glyphs
/// queued. Useful for profiling / sanity checks.
pub fn render_gui_wgpu(
    jobs: &[(GuiDocument, GuiComponent)],
    batch: &mut WgpuSpriteBatch,
    fonts: Option<&FrameFonts<'_>>,
    screen_w: f32,
    screen_h: f32,
    mut flush: impl FnMut(&mut WgpuSpriteBatch, UiFlush<'_>),
) -> (usize, usize) {
    let mut quads = 0usize;
    let mut glyphs = 0usize;

    for (doc, gui_comp) in jobs {
        if !gui_comp.visible {
            continue;
        }

        // Layout is computed in top-left-origin screen space (y=0 at top),
        // but the caller's `ui_vp` is a Y-up GL ortho (`orthographic_rh_gl(
        // 0, screen_w, 0, screen_h, ...)`) — same one used by
        // `render_ui_wgpu`, whose world-space UI anchors are already Y-up.
        // To avoid a vertically-flipped GUI we flip the layout rects here,
        // at the renderer boundary, so the hit-test (`process_gui_clicks`)
        // and unit tests can keep operating in their natural top-down
        // convention while the draw matches the visible viewport.
        let screen_rect = LayoutRect {
            x: 0.0,
            y: 0.0,
            width: screen_w,
            height: screen_h,
        };
        let flip = |r: LayoutRect| crate::ui::layout::flip_y(r, screen_h);
        let layout: Vec<crate::ui::layout::LayoutEntry> = compute_layout(doc, screen_w, screen_h)
            .into_iter()
            .map(|e| crate::ui::layout::LayoutEntry {
                rect: flip(e.rect),
                // Clamp to the surface: `set_scissor_rect` panics on a rect
                // that pokes outside the attachment, and a clip anchored off
                // the top of the screen does exactly that.
                clip: e.clip.map(|c| flip(c).intersect(&screen_rect)),
                ..e
            })
            .collect();
        let (q, g) = render_gui_layout_wgpu(
            doc,
            &layout,
            batch,
            fonts,
            gui_comp,
            screen_h,
            &mut flush,
        );
        quads += q;
        glyphs += g;

        // Focus highlight: 4 thin rects around the focused node.
        if let Some(focused_id) = &gui_comp.focused_node_id
            && let Some(entry) = layout.iter().find(|e| &e.id == focused_id)
            && entry.opacity > 0.001
        {
            let rect = &entry.rect;
            let radius = doc.root.find_node(focused_id).map_or(0.0, |n| n.style.corner_radius) * entry.scale;
            let focus_color = Vec4::new(0.54, 0.71, 0.98, 0.8 * entry.opacity); // #89b4fa
            let bw = 2.0_f32;
            // A rounded node gets a ring that follows its corners, just
            // outside it (four square edges round a pill read as a box).
            if radius > 0.0 {
                let gap = bw * 1.5;
                let ring = LayoutRect {
                    x: rect.x - gap,
                    y: rect.y - gap,
                    width: rect.width + gap * 2.0,
                    height: rect.height + gap * 2.0,
                };
                let m = crate::ui::shapes::ShapeMesh::rounded_ring(ring, radius + gap, bw, focus_color.to_array());
                batch.begin();
                batch.draw_mesh(&m.positions, &m.uvq, &m.colors, &m.indices);
                flush(batch, UiFlush::Quad(None));
                quads += 1;
                continue;
            }
            let center = Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
            batch.begin();
            // Top edge
            batch.draw_sprite_uv(
                Vec2::new(center.x, rect.y - bw / 2.0),
                Vec2::new(rect.width + bw * 2.0, bw),
                0.0, focus_color, [0.0, 0.0, 1.0, 1.0],
            );
            // Bottom edge
            batch.draw_sprite_uv(
                Vec2::new(center.x, rect.y + rect.height + bw / 2.0),
                Vec2::new(rect.width + bw * 2.0, bw),
                0.0, focus_color, [0.0, 0.0, 1.0, 1.0],
            );
            // Left edge
            batch.draw_sprite_uv(
                Vec2::new(rect.x - bw / 2.0, center.y),
                Vec2::new(bw, rect.height),
                0.0, focus_color, [0.0, 0.0, 1.0, 1.0],
            );
            // Right edge
            batch.draw_sprite_uv(
                Vec2::new(rect.x + rect.width + bw / 2.0, center.y),
                Vec2::new(bw, rect.height),
                0.0, focus_color, [0.0, 0.0, 1.0, 1.0],
            );
            flush(batch, UiFlush::Quad(None));
            quads += 1;
        }
    }

    (quads, glyphs)
}

/// Walk a computed GUI layout and emit batches per node + widget kind.
#[allow(clippy::too_many_arguments)]
/// What the next GUI draw needs bound.
///
/// `White` is the 1x1 fallback every panel, border and button chrome uses, so
/// they are all interchangeable within a run — which is most of a GUI.
#[derive(PartialEq)]
enum UiTarget {
    White,
    Image(String),
    Font(String),
}

/// Coalesces consecutive GUI draws that share a binding into one flush.
///
/// The GUI renderer used to wrap every single node in `batch.begin()` /
/// `flush(...)`, which is **one draw call per node** — an equipment panel of
/// 110 nodes cost 110 draws, several times the entire game world in the same
/// frame (sprites batch by texture and land around 14). Panel backgrounds,
/// borders and button chrome all bind the same white fallback, so consecutive
/// ones can share a batch.
///
/// Draw order is preserved because only **adjacent** same-target draws merge.
/// Anything that changes the binding — an image, a font atlas, a scissor rect
/// — ends the run, and `end` is called before every clip change and at the
/// end of the document.
#[derive(Default)]
struct UiRun {
    cur: Option<UiTarget>,
    /// Flushes actually issued. This is what `render_gui_wgpu` reports as its
    /// quad count, so the profiler's number is the real draw count rather
    /// than the node count.
    flushes: usize,
}

impl UiRun {
    /// Ensure `t` is what the batch is accumulating for, flushing first if the
    /// binding changes. A no-op when the run already matches, which is the
    /// case this exists for.
    /// Would `t` continue the open run?
    ///
    /// This one predicate is the whole optimization, and the only thing here
    /// that can silently corrupt a frame: say yes too eagerly and draws merge
    /// into a batch flushed with the wrong binding — two different images
    /// would render as whichever one won. Unit-tested below for that reason.
    fn continues(&self, t: &UiTarget) -> bool {
        self.cur.as_ref() == Some(t)
    }

    fn want(
        &mut self,
        batch: &mut WgpuSpriteBatch,
        flush: &mut impl FnMut(&mut WgpuSpriteBatch, UiFlush<'_>),
        t: UiTarget,
    ) {
        if self.continues(&t) {
            return; // keep accumulating into the open batch
        }
        self.end(batch, flush);
        batch.begin();
        self.cur = Some(t);
    }

    /// Flush whatever is queued and close the run. Idempotent.
    fn end(
        &mut self,
        batch: &mut WgpuSpriteBatch,
        flush: &mut impl FnMut(&mut WgpuSpriteBatch, UiFlush<'_>),
    ) {
        let Some(t) = self.cur.take() else { return };
        match t {
            UiTarget::White => flush(batch, UiFlush::Quad(None)),
            UiTarget::Image(id) => flush(batch, UiFlush::image(&id)),
            UiTarget::Font(id) => flush(batch, UiFlush::Font(&id)),
        }
        self.flushes += 1;
    }
}

fn render_gui_layout_wgpu(
    doc: &GuiDocument,
    layout: &[crate::ui::layout::LayoutEntry],
    batch: &mut WgpuSpriteBatch,
    fonts: Option<&FrameFonts<'_>>,
    gui: &GuiComponent,
    screen_h: f32,
    flush: &mut impl FnMut(&mut WgpuSpriteBatch, UiFlush<'_>),
) -> (usize, usize) {
    use crate::ui::shapes::ShapeMesh;
    let variables = &gui.variables;
    // A shape mesh through the batch: rounded panels, rings, shadows.
    let draw_shape = |batch: &mut WgpuSpriteBatch, m: ShapeMesh| {
        batch.draw_mesh(&m.positions, &m.uvq, &m.colors, &m.indices);
    };
    // Entries carry the clip in the same y-up space as their rects (so the
    // cull below compares like with like); the scissor needs it back in
    // framebuffer space.
    let to_framebuffer = |c: LayoutRect| crate::ui::layout::flip_y(c, screen_h);

    let mut glyphs = 0usize;
    // Scissor state is sticky on the render pass, so only emit a change when
    // the clip actually differs from what is already set.
    let mut active_clip: Option<LayoutRect> = None;
    // Coalesces consecutive draws that share a binding — see `UiRun`.
    let mut run = UiRun::default();

    for entry in layout {
        let (node_id, rect) = (&entry.id, &entry.rect);
        let Some(node) = doc.root.find_node(node_id) else {
            continue;
        };
        if !node.style.visible {
            continue;
        }
        // Fully scrolled out of its container: nothing to draw, and skipping
        // it keeps degenerate scissor rects off the pass.
        if entry.clip.is_some() && entry.visible_rect().is_empty() {
            continue;
        }
        if entry.clip != active_clip {
            active_clip = entry.clip;
            run.end(batch, flush);
            flush(batch, UiFlush::Clip(active_clip.map(to_framebuffer)));
        }

        let center = Vec2::new(rect.x + rect.width * 0.5, rect.y + rect.height * 0.5);
        let size = Vec2::new(rect.width, rect.height);
        // Inherited opacity and scale (Sep 2026): every colour below goes
        // through `fade`, every size in pixels is multiplied by `k`.
        let (op, k) = (entry.opacity, entry.scale);
        if op <= 0.001 {
            continue;
        }
        let fade = |c: [f32; 4]| Vec4::new(c[0], c[1], c[2], c[3] * op);
        let faded = |c: [f32; 4]| [c[0], c[1], c[2], c[3] * op];
        let radius = node.style.corner_radius * k;
        // A filled rect of this node's shape: rounded when it asks.
        let fill = |batch: &mut WgpuSpriteBatch, r: LayoutRect, c: [f32; 4], rad: f32| {
            if rad > 0.0 {
                draw_shape(batch, ShapeMesh::rounded_rect(r, rad, faded(c), [0.0, 0.0, 1.0, 1.0]));
            } else {
                batch.draw_sprite_uv(
                    Vec2::new(r.x + r.width * 0.5, r.y + r.height * 0.5),
                    Vec2::new(r.width, r.height),
                    0.0, fade(c), [0.0, 0.0, 1.0, 1.0],
                );
            }
        };

        // Shadow, under everything the node draws. Its offset is y-down, as
        // a designer means it; these rects are y-up.
        if let Some(sh) = node.style.shadow
            && sh.color[3] > 0.0
        {
            let r = LayoutRect { x: rect.x + sh.offset[0] * k, y: rect.y - sh.offset[1] * k, ..*rect };
            run.want(batch, flush, UiTarget::White);
            draw_shape(batch, ShapeMesh::soft_rounded_rect(r, radius, sh.blur.max(0.0) * k, faded(sh.color)));
        }

        // Background
        let bg = node.style.bg_color;
        if bg[3] > 0.0 {
            run.want(batch, flush, UiTarget::White);
            fill(batch, *rect, bg, radius);
        }

        // Scrollbar for an `Overflow::Scroll` container with somewhere to go.
        // Drawn by the engine rather than assembled from nodes in each `.gui`
        // file: the indicator has to agree with the offset the wheel handler
        // clamped to, and every runtime shares this one function.
        if let Some((max_x, _)) = entry.scroll_max
            && max_x > 0.0
        {
            let scrolled = node.style.scroll_x.clamp(0.0, max_x);
            let track_h = 6.0_f32;
            let inset = 2.0_f32;
            let track_x = rect.x + inset;
            let track_y = rect.y + inset;
            let track_w = (rect.width - inset * 2.0).max(0.0);
            let content_w = rect.width + max_x;
            let thumb_w = (rect.width / content_w * track_w).clamp(16.0_f32.min(track_w), track_w);
            let travel = (track_w - thumb_w).max(0.0);
            let thumb_x = track_x + (scrolled / max_x) * travel;
            run.want(batch, flush, UiTarget::White);
            batch.draw_sprite_uv(
                Vec2::new(track_x + track_w * 0.5, track_y + track_h * 0.5),
                Vec2::new(track_w, track_h),
                0.0,
                Vec4::new(1.0, 1.0, 1.0, 0.08),
                [0.0, 0.0, 1.0, 1.0],
            );
            batch.draw_sprite_uv(
                Vec2::new(thumb_x + thumb_w * 0.5, track_y + track_h * 0.5),
                Vec2::new(thumb_w, track_h),
                0.0,
                Vec4::new(0.66, 0.76, 0.95, 0.85),
                [0.0, 0.0, 1.0, 1.0],
            );
        }
        if let Some((_, max_y)) = entry.scroll_max
            && max_y > 0.0
        {
            let scrolled = node.style.scroll_y.clamp(0.0, max_y);
            let track_w = 6.0_f32;
            let inset = 2.0_f32;
            let track_x = rect.x + rect.width - track_w - inset;
            let track_y = rect.y + inset;
            let track_h = (rect.height - inset * 2.0).max(0.0);
            // Thumb length is the visible fraction of the content.
            let content_h = rect.height + max_y;
            let thumb_h = (rect.height / content_h * track_h).clamp(16.0_f32.min(track_h), track_h);
            let travel = (track_h - thumb_h).max(0.0);
            // Rects here are already y-flipped for the GL-style ortho, so a
            // scroll offset of 0 puts the thumb at the *top* — the high y.
            let thumb_y = track_y + travel - (scrolled / max_y) * travel;
            run.want(batch, flush, UiTarget::White);
            batch.draw_sprite_uv(
                Vec2::new(track_x + track_w * 0.5, track_y + track_h * 0.5),
                Vec2::new(track_w, track_h),
                0.0,
                Vec4::new(1.0, 1.0, 1.0, 0.08),
                [0.0, 0.0, 1.0, 1.0],
            );
            batch.draw_sprite_uv(
                Vec2::new(track_x + track_w * 0.5, thumb_y + thumb_h * 0.5),
                Vec2::new(track_w, thumb_h),
                0.0,
                Vec4::new(0.66, 0.76, 0.95, 0.85),
                [0.0, 0.0, 1.0, 1.0],
            );
        }

        // Widget-specific rendering — verbatim port of gl `render_gui_layout`.
        match &node.node_type {
            GuiNodeType::Panel => {} // Background already drawn above.

            GuiNodeType::Text { text, font_id, font_size, color, alignment, rich, .. } => {
                if let Some((fd, font_tex)) = fonts.map(|f| f.resolve(font_id)) {
                    let markup = substitute_template_wgpu(text, variables);
                    // Rich text lays out (and is aligned by) its plain text.
                    let rt = rich.then(|| {
                        crate::renderer::rich_text::parse(&markup, crate::renderer::rich_text::icon_spaces(fd))
                    });
                    let resolved = rt.as_ref().map_or_else(|| markup.clone(), |r| r.plain.clone());
                    if !resolved.is_empty() {
                        // `alignment` used to be ignored here — every GUI Text
                        // node drew from `rect.x` regardless, so a document that
                        // asked for Center silently rendered left-aligned. The
                        // older `render_ui_wgpu` path honoured it, which is why
                        // the field looked like it worked.
                        //
                        // Each line is placed about `text_x` by its own width
                        // (`text_layout`), as world text's are; summing the
                        // whole string's advances, as this did until Oct
                        // 2026, centred a two-line label by both lines' length.
                        let (text_x, align) = match alignment {
                            GuiTextAlign::Left => (rect.x, TextAlign::Left),
                            GuiTextAlign::Center => (rect.x + rect.width * 0.5, TextAlign::Center),
                            GuiTextAlign::Right => (rect.x + rect.width, TextAlign::Right),
                        };
                        run.want(batch, flush, UiTarget::Font(font_tex.to_string()));
                        let scale = *font_size * k / fd.pixel_size;
                        let text_pos =
                            Vec2::new(text_x, centered_line_top(fd, &resolved, scale, center.y));
                        match &rt {
                            Some(rt) => {
                                let all = (usize::MAX, 0.0);
                                let c = fade(*color);
                                glyphs += queue_rich_ui(fd, batch, rt, all, text_pos, scale, c.to_array(), None, align);
                                let side = fd.line_height * scale * 0.9;
                                for (id, spot) in rich_icon_spots(fd, rt, all, text_pos, scale, 0.0, None, align) {
                                    run.want(batch, flush, UiTarget::Image(markup[id].to_string()));
                                    batch.draw_sprite_uv(spot, Vec2::splat(side), 0.0, Vec4::new(1.0, 1.0, 1.0, c.w), FULL_UV);
                                }
                            }
                            None => {
                                glyphs += queue_text_glyphs_wrapped(fd, batch, &resolved, text_pos, scale, fade(*color), None, align)
                            }
                        }
                    }
                }
            }

            GuiNodeType::Image { texture_id, tint, uv_rect } => {
                let uv = uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
                run.want(batch, flush, UiTarget::Image(texture_id.to_string()));
                if radius > 0.0 {
                    draw_shape(batch, ShapeMesh::rounded_rect(*rect, radius, faded(*tint), uv));
                } else {
                    batch.draw_sprite_uv(center, size, 0.0, fade(*tint), uv);
                }
            }

            GuiNodeType::Button {
                label, font_id, font_size, text_color, normal_color, hover_color, pressed_color, disabled, alignment,
                hover_scale,
            } => {
                // Half brightness when disabled, matching the widget-based
                // `UIButton` (`button_display_color`). Before Sep 2026 the
                // flag was authored, saved and loaded but drawn no
                // differently, so a shop could not show what it refused.
                // And hover / pressed are drawn since Sep 2026 - `navigate_ui`
                // tracks the pointer; the colours were authored and unread.
                let hovered = !*disabled && gui.hovered_node_id.as_deref() == Some(node.id.as_str());
                // Eased: the colour and the size come on together.
                let t = if hovered { let t = gui.hover_t.clamp(0.0, 1.0); t * t * (3.0 - 2.0 * t) } else { 0.0 };
                let mix = |a: [f32; 4], b: [f32; 4]| std::array::from_fn::<f32, 4, _>(|i| a[i] + (b[i] - a[i]) * t);
                let (bg, fg) = if *disabled {
                    (dimmed(*normal_color), dimmed(*text_color))
                } else if hovered && gui.pointer_down {
                    (*pressed_color, *text_color)
                } else {
                    (mix(*normal_color, *hover_color), *text_color)
                };
                let grow = 1.0 + (hover_scale - 1.0) * t;
                let rect = &LayoutRect {
                    x: center.x - rect.width * grow * 0.5,
                    y: center.y - rect.height * grow * 0.5,
                    width: rect.width * grow,
                    height: rect.height * grow,
                };
                let k = k * grow;
                let radius = radius * grow;
                run.want(batch, flush, UiTarget::White);
                fill(batch, *rect, bg, radius);
                if let Some((fd, font_tex)) = fonts.map(|f| f.resolve(font_id)) {
                    let resolved = substitute_template_wgpu(label, variables);
                    if !resolved.is_empty() {
                        run.want(batch, flush, UiTarget::Font(font_tex.to_string()));
                        let scale = *font_size * k / fd.pixel_size;
                        let tw: f32 = resolved
                            .chars()
                            .filter_map(|ch| fd.glyphs.iter().find(|g| g.ch == ch).or_else(|| fd.glyphs.iter().find(|g| g.ch == '?')))
                            .map(|g| g.advance)
                            .sum::<f32>()
                            * scale;
                        let text_x = match alignment {
                            GuiTextAlign::Left => rect.x + 4.0 * k,
                            GuiTextAlign::Center => rect.x + (rect.width - tw) * 0.5,
                            GuiTextAlign::Right => rect.x + rect.width - tw - 4.0 * k,
                        };
                        let text_pos = Vec2::new(text_x, centered_line_top(fd, &resolved, scale, center.y));
                        glyphs += queue_text_glyphs(fd, batch, &resolved, text_pos, scale, fade(fg));
                    }
                }
            }

            GuiNodeType::ProgressBar { value, min, max, fg_color, bg_color, .. } => {
                let range = (max - min).max(f32::EPSILON);
                let fraction = ((value - min) / range).clamp(0.0, 1.0);
                run.want(batch, flush, UiTarget::White);
                fill(batch, *rect, *bg_color, radius);
                if fraction > 0.0 {
                    let fill_w = rect.width * fraction;
                    run.want(batch, flush, UiTarget::White);
                    fill(batch, LayoutRect { width: fill_w, ..*rect }, *fg_color, radius.min(fill_w * 0.5));
                }
            }

            GuiNodeType::Slider { value, min, max, track_color, thumb_color, thumb_size } => {
                let range = (max - min).max(f32::EPSILON);
                let fraction = ((value - min) / range).clamp(0.0, 1.0);
                let track_h = (rect.height * 0.3).max(4.0);
                run.want(batch, flush, UiTarget::White);
                batch.draw_sprite_uv(center, Vec2::new(rect.width, track_h), 0.0, fade(*track_color), [0.0, 0.0, 1.0, 1.0]);
                let thumb_x = rect.x + fraction * rect.width;
                run.want(batch, flush, UiTarget::White);
                batch.draw_sprite_uv(
                    Vec2::new(thumb_x, center.y),
                    Vec2::new(*thumb_size, rect.height),
                    0.0, fade(*thumb_color), [0.0, 0.0, 1.0, 1.0],
                );
            }

            GuiNodeType::Toggle { checked, on_color, off_color, thumb_color } => {
                let track_color = if *checked { on_color } else { off_color };
                // Rounded when the node is (a pill switch), square otherwise.
                run.want(batch, flush, UiTarget::White);
                fill(batch, *rect, *track_color, radius);
                let thumb_w = rect.width * 0.45;
                let thumb_x = if *checked {
                    rect.x + rect.width - thumb_w * 0.5 - 2.0
                } else {
                    rect.x + thumb_w * 0.5 + 2.0
                };
                let thumb = LayoutRect { x: thumb_x - thumb_w * 0.5, y: rect.y + 2.0, width: thumb_w, height: rect.height - 4.0 };
                fill(batch, thumb, *thumb_color, (radius - 2.0).clamp(0.0, thumb.height.min(thumb.width) * 0.5));
            }

            GuiNodeType::TextInput { text, placeholder, font_id, font_size, color, bg_color, password, edit, .. } => {
                run.want(batch, flush, UiTarget::White);
                batch.draw_sprite_uv(center, size, 0.0, fade(*bg_color), [0.0, 0.0, 1.0, 1.0]);
                if let Some((fd, font_tex)) = fonts.map(|f| f.resolve(font_id)) {
                    // The caret, the selection, a long line scrolled to keep
                    // the caret in sight (`ui::text_edit`). The view is
                    // recorded top-left, as the layout and the clicks are.
                    let scale = *font_size * k / fd.pixel_size;
                    let focused = gui.focused_node_id.as_deref() == Some(node_id.as_str());
                    let field = [rect.x, rect.y, rect.width, rect.height];
                    let view = [rect.x, screen_h - rect.y - rect.height, rect.width, rect.height];
                    let plan = plan_text_field(
                        fd, text, placeholder, *password, edit, focused,
                        crate::ui::text_edit::gui_key(&gui.gui_id, node_id), field, view, scale, fade(*color),
                    );
                    if let Some((at, sz)) = plan.selection {
                        run.want(batch, flush, UiTarget::White);
                        batch.draw_sprite_uv(at, sz, 0.0, Vec4::new(color[0], color[1], color[2], color[3] * 0.3 * op), [0.0, 0.0, 1.0, 1.0]);
                    }
                    if !plan.shown.is_empty() {
                        run.want(batch, flush, UiTarget::Font(font_tex.to_string()));
                        glyphs += queue_text_glyphs(fd, batch, &plan.shown, plan.text_pos, scale, plan.color);
                    }
                    for (at, sz) in plan.caret.into_iter().chain(plan.underline) {
                        run.want(batch, flush, UiTarget::White);
                        batch.draw_sprite_uv(at, sz, 0.0, fade(*color), [0.0, 0.0, 1.0, 1.0]);
                    }
                }
            }

            GuiNodeType::Circle { fill_color, stroke_color, stroke_width } => {
                // Round, via the built-in anti-aliased disc. This used to draw
                // the fill as a plain quad and the "stroke" as four straight
                // edges, i.e. a rectangle with a border — the one node type
                // whose entire purpose is its shape looked exactly like a
                // Panel.
                //
                // A stroke is the outer disc drawn in `stroke_color` with the
                // fill disc inset on top. That reads correctly for a filled
                // circle with an outline; a *hollow* ring (fill alpha 0) shows
                // the stroke disc filled instead, which needs a ring texture or
                // an SDF shader to do properly.
                let disc = crate::asset::CIRCLE_TEXTURE_ID;
                let stroke = if stroke_color[3] > 0.0 { stroke_width.max(0.0) } else { 0.0 };
                if stroke > 0.0 {
                    run.want(batch, flush, UiTarget::Image(disc.to_string()));
                    batch.draw_sprite_uv(center, size, 0.0, fade(*stroke_color), [0.0, 0.0, 1.0, 1.0]);
                }
                if fill_color[3] > 0.0 {
                    let inner = Vec2::new(
                        (size.x - stroke * 2.0).max(0.0),
                        (size.y - stroke * 2.0).max(0.0),
                    );
                    run.want(batch, flush, UiTarget::Image(disc.to_string()));
                    batch.draw_sprite_uv(center, inner, 0.0, fade(*fill_color), [0.0, 0.0, 1.0, 1.0]);
                }
            }
        }
        // Border: a rounded ring, or 4 edges in one batch - drawn over the
        // widget, or a button's own body hides it (a selected inventory
        // slot outlined in gold showed no outline at all).
        if node.style.border_width > 0.0 && node.style.border_color[3] > 0.0 && radius > 0.0 {
            run.want(batch, flush, UiTarget::White);
            draw_shape(batch, ShapeMesh::rounded_ring(*rect, radius, node.style.border_width * k, faded(node.style.border_color)));
        } else if node.style.border_width > 0.0 && node.style.border_color[3] > 0.0 {
            let bw = node.style.border_width * k;
            let bc = fade(node.style.border_color);
            run.want(batch, flush, UiTarget::White);
            batch.draw_sprite_uv(
                Vec2::new(center.x, rect.y + bw * 0.5),
                Vec2::new(rect.width, bw),
                0.0, bc, [0.0, 0.0, 1.0, 1.0],
            );
            batch.draw_sprite_uv(
                Vec2::new(center.x, rect.y + rect.height - bw * 0.5),
                Vec2::new(rect.width, bw),
                0.0, bc, [0.0, 0.0, 1.0, 1.0],
            );
            batch.draw_sprite_uv(
                Vec2::new(rect.x + bw * 0.5, center.y),
                Vec2::new(bw, rect.height),
                0.0, bc, [0.0, 0.0, 1.0, 1.0],
            );
            batch.draw_sprite_uv(
                Vec2::new(rect.x + rect.width - bw * 0.5, center.y),
                Vec2::new(bw, rect.height),
                0.0, bc, [0.0, 0.0, 1.0, 1.0],
            );
        }

    }

    // Close the final run. UNCONDITIONALLY: this used to sit inside the
    // `active_clip` branch below, which meant any document without a scroll
    // container — almost all of them — never flushed its last batch and
    // silently lost its trailing draws.
    run.end(batch, flush);
    // Leave the pass unscissored for whatever draws next (the focus
    // highlight, the next document, the debug overlay).
    if active_clip.is_some() {
        flush(batch, UiFlush::Clip(None));
    }
    (run.flushes, glyphs)
}

/// Verbatim copy of the private `substitute_template` from gl `systems.rs`.
/// Replaces `{{key}}` placeholders in `text` with values from `variables`.
fn substitute_template_wgpu(
    text: &str,
    variables: &std::collections::HashMap<String, serde_json::Value>,
) -> String {
    if !text.contains("{{") || variables.is_empty() {
        return text.to_string();
    }
    let mut result = text.to_string();
    for (key, value) in variables {
        let placeholder = format!("{{{{{}}}}}", key);
        if result.contains(&placeholder) {
            let replacement = match value {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                serde_json::Value::Bool(b) => b.to_string(),
                serde_json::Value::Null => String::new(),
                other => other.to_string(),
            };
            result = result.replace(&placeholder, &replacement);
        }
    }
    result
}


/// Render shadow silhouettes for every `Transform + SpriteComponent + ShadowCaster`
/// entity, bucketed by sprite texture for one draw call per texture.
///
/// Phase 3.c-4 Step 5e (shadows half): wgpu twin of
/// [`crate::ecs::systems::render_shadows`]. Builds a shadow drawable per
/// caster (position offset by `offset * height`, size scaled by
/// `transform.scale * shadow.scale`, color tinted by the caster's color),
/// sorts by `(z_order, texture_id)`, and flushes one batch per texture.
///
/// Caller responsibilities (`flush_group(batch, &str)`):
/// - Resolve the wgpu texture for `texture_id` (typically via
///   `AssetManager::ensure_wgpu_texture`, falling back to default texture).
/// - Call `batch.flush(...)` with the camera VP.
///
/// Should be called BEFORE `render_sprites_wgpu` / `_multi` so shadows draw
/// behind sprites.
///
/// **Limitation vs gl path:** the gl `render_shadows` switches the GL blend
/// state to `BlendMode::Multiply` for the duration of the pass, which makes
/// shadows darken what they overlay. The wgpu sprite pipeline is hardcoded
/// to alpha-blend in this slice, so wgpu shadows will overlay (composite on
/// top with alpha) rather than multiply — they'll still look like dark
/// translucent quads but won't darken bright backgrounds the way the gl
/// path does. Proper multiply-blend pipeline arrives alongside additive in
/// Phase 4 (per-blend-mode pipeline cache). Documented inline.
///
/// Returns the total number of shadow quads queued.
pub fn render_shadows_wgpu(
    world: &World,
    sort_layers: &SortLayers,
    batch: &mut WgpuSpriteBatch,
    default_size: Vec2,
    mut flush_group: impl FnMut(&mut WgpuSpriteBatch, &str),
) -> usize {
    // Lightweight intermediate, mirrors gl `ShadowDrawable`. We keep
    // `texture_id` as a String here because wgpu callers resolve textures
    // by id-string (no GL handle to compare on like the gl path does).
    /// Borrows its texture id from the sprite rather than owning a copy:
    /// cloning a `String` per shadow caster per frame is an allocation for a
    /// value the renderer only reads. See `docs/architecture-debt.md` #1.
    struct ShadowDrawable<'a> {
        position: Vec2,
        size: Vec2,
        rotation: f32,
        skew: f32,
        color: Vec4,
        uv: [f32; 4],
        anchor: [f32; 2],
        z_order: i32,
        texture_id: &'a str,
    }

    let mut shadow_query =
        world.query::<(&Transform, &SpriteComponent, &ShadowCaster)>();
    let mut shadows: Vec<ShadowDrawable> = shadow_query
        .iter()
        .filter(|(_, (_, _, sc))| sc.enabled)
        .map(|(_, (transform, sprite, shadow))| {
            let offset = Vec2::new(
                shadow.offset[0] * shadow.height,
                shadow.offset[1] * shadow.height,
            );
            let position = transform.position + offset;
            let uv = sprite.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
            let base = sprite.drawn_size(default_size);
            let shadow_scale = Vec2::new(shadow.scale[0], shadow.scale[1]);
            let size = base * transform.scale * shadow_scale;
            ShadowDrawable {
                position,
                size,
                rotation: transform.rotation,
                skew: shadow.skew,
                color: Vec4::from(shadow.color),
                uv,
                anchor: sprite.anchor,
                z_order: sort_layers.resolve(sprite.sort_layer.as_deref(), sprite.z_order),
                // An empty id means "tint the white pixel"; resolved here so
                // the batch groups on the id the flush will actually bind.
                texture_id: crate::asset::resolve_texture_id(&sprite.texture_id),
            }
        })
        .collect();

    if shadows.is_empty() {
        return 0;
    }

    // Sort by z_order then texture_id for batch grouping (gl path sorts by
    // glow::Texture handle; we use the string id since wgpu textures are
    // looked up lazily inside the flush callback).
    shadows.sort_by(|a, b| {
        a.z_order
            .cmp(&b.z_order)
            .then_with(|| a.texture_id.cmp(b.texture_id))
    });

    let mut total = 0usize;
    let mut i = 0;
    while i < shadows.len() {
        let group_tex = shadows[i].texture_id;
        let group_z = shadows[i].z_order;
        batch.begin();
        let mut count = 0usize;
        while i < shadows.len()
            && shadows[i].texture_id == group_tex
            && shadows[i].z_order == group_z
        {
            if batch.is_full() {
                flush_group(batch, group_tex);
                total += count;
                count = 0;
                batch.begin();
            }
            let s = &shadows[i];
            batch.draw_sprite_anchored_skewed(
                s.position,
                s.size,
                s.rotation,
                s.skew,
                s.color,
                s.uv,
                s.anchor,
            );
            count += 1;
            i += 1;
        }
        if count > 0 {
            flush_group(batch, group_tex);
            total += count;
        }
    }

    total
}

/// **No runtime calls this.** Skinned meshes are drawn by the sprite pass
/// since Sep 2026 (`InterleavedItem::Skinned`), sorted by `z_order`; this
/// pass drew after every sprite, so no z could put a creature under anything.
/// Calling it as well draws every mesh twice.
///
/// Slice 3.5.E — render every `SkinnedMesh` attached to a
/// `SkeletonRoot` entity. CPU-skins each vertex against the rig's
/// resolved `WorldTransform`s, then pushes each triangle through the
/// sprite batch via `draw_triangle` (degenerate-quad reuses the
/// static quad index buffer). Meshes are sorted by `z_order` then
/// `texture_id` and flushed per texture group through the supplied
/// callback — same protocol as [`render_sprites_wgpu_multi`].
/// Returns the total number of triangles queued.
#[cfg(feature = "skinned_mesh")]
#[deprecated = "skinned meshes are drawn by the sprite pass, sorted by z_order; this draws them a second time over every sprite"]
pub fn render_skinned_meshes_wgpu(
    world: &World,
    sort_layers: &SortLayers,
    batch: &mut WgpuSpriteBatch,
    mut flush_group: impl FnMut(&mut WgpuSpriteBatch, &str),
) -> usize {
    /// Borrows its texture id from the mesh: the id is only read, and cloning
    /// a `String` per mesh per frame is an allocation for nothing.
    struct Item<'a> {
        root: hecs::Entity,
        z: i32,
        tex: &'a str,
    }

    let mut mesh_query =
        world.query::<(&SkinnedMesh, &crate::ecs::components::SkeletonRoot)>();
    let mut items: Vec<Item> = mesh_query
        .iter()
        .map(|(entity, (mesh, _))| Item {
            root: entity,
            z: sort_layers.resolve(None, mesh.z_order),
            tex: crate::asset::resolve_texture_id(&mesh.texture_id),
        })
        .collect();
    if items.is_empty() {
        return 0;
    }
    items.sort_by(|a, b| a.z.cmp(&b.z).then_with(|| a.tex.cmp(b.tex)));

    let mut total = 0usize;
    let mut i = 0;
    while i < items.len() {
        let group_tex = items[i].tex;
        let group_z = items[i].z;
        batch.begin();
        let mut count = 0usize;
        while i < items.len() && items[i].tex == group_tex && items[i].z == group_z {
            let root = items[i].root;
            // Snapshot the mesh data we need before doing the
            // skinning pass, so the world borrow released here
            // doesn't tangle with `compute_skinned_positions`'s own
            // immutable borrow of the world.
            let (indices, uvs, color) = {
                let mesh = world.get::<&SkinnedMesh>(root).expect("mesh");
                (
                    mesh.indices.clone(),
                    mesh.vertices.iter().map(|v| v.uv).collect::<Vec<_>>(),
                    glam::Vec4::from(mesh.color),
                )
            };
            let positions = {
                let mesh = world.get::<&SkinnedMesh>(root).expect("mesh");
                compute_skinned_positions(&mesh, world, root)
            };
            let mut tri = 0;
            while tri + 2 < indices.len() {
                if batch.is_full() {
                    flush_group(batch, group_tex);
                    total += count;
                    count = 0;
                    batch.begin();
                }
                let i0 = indices[tri] as usize;
                let i1 = indices[tri + 1] as usize;
                let i2 = indices[tri + 2] as usize;
                if i0 < positions.len() && i1 < positions.len() && i2 < positions.len() {
                    batch.draw_triangle(
                        positions[i0],
                        positions[i1],
                        positions[i2],
                        uvs[i0],
                        uvs[i1],
                        uvs[i2],
                        color,
                    );
                    count += 1;
                }
                tri += 3;
            }
            i += 1;
        }
        if count > 0 {
            flush_group(batch, group_tex);
            total += count;
        }
    }
    total
}
/// Internal draw item used by [`render_sprites_with_shadows_wgpu_materialized`]
/// to interleave shadow and sprite quads by z-order.
///
/// Kept private so callers can't accidentally couple to the streaming layout;
/// the sort key extraction is exposed to tests via [`sort_key_for_test`].
enum InterleavedItem<'a> {
    /// A `ShadowCaster` quad rendered through the multiply pipeline. Carries
    /// the same geometry payload the standalone [`render_shadows_wgpu`] path
    /// would push (`draw_sprite_anchored_skewed`).
    Shadow {
        z: i32,
        /// Its sorting group (`ecs::sorting_group`).
        g: crate::ecs::sorting_group::SortGroup,
        tex: &'a str,
        pos: Vec2,
        size: Vec2,
        rot: f32,
        skew: f32,
        color: Vec4,
        uv: [f32; 4],
        anchor: [f32; 2],
    },
    /// A regular `SpriteComponent` quad (or multi-quad via `SpriteFill`)
    /// rendered through the standard alpha-blend pipeline (optionally a
    /// material-graph-built one).
    Sprite {
        z: i32,
        /// Its sorting group (`ecs::sorting_group`).
        g: crate::ecs::sorting_group::SortGroup,
        tex: &'a str,
        mat: Option<&'a str>,
        /// The material instance's parameters, packed as bits: part of the
        /// batching key, so only a different value splits a batch.
        params: Params,
        /// [`sprite_blend`] of its material - also part of the batching key.
        blend: crate::renderer::material::BlendMode,
        /// Drawn as its shape in black ([`WorldLayer`]).
        cutout: bool,
        /// Its mask in the frame's [`MaskTable`], 0 for none.
        mask: u32,
        transform: WorldTransform,
        sprite: &'a SpriteComponent,
    },
    /// A world `TextComponent`, drawn as glyph quads from its font's atlas -
    /// the atlas is just another texture group. Text used to be its own pass
    /// after every sprite pass, so a label at z 5 drew over a sprite at z 50
    /// and `z_order` on text only ordered text against text.
    Text {
        z: i32,
        /// Its sorting group (`ecs::sorting_group`).
        g: crate::ecs::sorting_group::SortGroup,
        tex: &'a str,
        font: &'a FontData,
        text: &'a TextComponent,
        position: Vec2,
        scale: f32,
        rotation: f32,
        blend: crate::renderer::material::BlendMode,
        cutout: bool,
        mask: u32,
        /// The markup parsed, for a `rich` text, and how much of it the
        /// typewriter shows (characters, and how far into the next).
        rich: Option<std::rc::Rc<crate::renderer::rich_text::RichText>>,
        visible: (usize, f32),
        /// Which layer of the text this is: 0 its shadow, 1 its outline, 2
        /// the text - drawn in that order, every text's shadow under every
        /// text of its z.
        layer: u8,
        /// The layer's one colour (a shadow, an outline); `None` for the text.
        solid: Option<Vec4>,
        /// Where the layer is moved to, in world units.
        nudge: Vec2,
        /// The distance-field edge moved out and softened (`sprite.wgsl`'s
        /// `params[0]`), or for a bitmap font the copies to draw round it.
        style: TextStyle,
    },
    /// A rich text's inline image (`[icon=...]`): a quad of its own
    /// texture over the spaces the layout left for it, sorted with its text.
    Icon {
        z: i32,
        /// Its sorting group (`ecs::sorting_group`).
        g: crate::ecs::sorting_group::SortGroup,
        tex: &'a str,
        center: Vec2,
        size: Vec2,
        rotation: f32,
        color: Vec4,
        blend: crate::renderer::material::BlendMode,
        cutout: bool,
        mask: u32,
    },
    /// A [`Mesh2D`]: sorted and batched exactly as a sprite of its texture
    /// and material, which is the whole reason it is drawn here and not in
    /// a pass of its own.
    Mesh {
        z: i32,
        /// Its sorting group (`ecs::sorting_group`).
        g: crate::ecs::sorting_group::SortGroup,
        tex: &'a str,
        mat: Option<&'a str>,
        params: Params,
        blend: crate::renderer::material::BlendMode,
        cutout: bool,
        mask: u32,
        transform: WorldTransform,
        mesh: &'a Mesh2D,
    },
    /// A [`VectorPath`], tessellated for this frame and drawn against the
    /// white texture - sorted with everything else, where it used to be a
    /// pass of its own after every sprite.
    Path {
        z: i32,
        /// Its sorting group (`ecs::sorting_group`).
        g: crate::ecs::sorting_group::SortGroup,
        transform: WorldTransform,
        parts: Vec<crate::renderer::vector_path::PathPart>,
        blend: crate::renderer::material::BlendMode,
        cutout: bool,
        mask: u32,
    },
    /// A GPU particle emitter (`particles::gpu`): drawn alone, by the batch,
    /// in one instanced call at its z.
    Gpu {
        z: i32,
        /// Its sorting group (`ecs::sorting_group`).
        g: crate::ecs::sorting_group::SortGroup,
        tex: &'a str,
        blend: crate::renderer::material::BlendMode,
        /// Boxed: the uniform is ~620 bytes, and an enum is as large as its
        /// largest variant - inline, it made every sprite's item that size,
        /// six megabytes written and read again per pass of ten thousand.
        /// With it, a stateful emitter's key (`particles::gpu_state`).
        uniform: Box<(crate::particles::gpu::GpuParticleUniform, Option<u64>)>,
        count: u32,
    },
    /// A tile of a `y_sort` tilemap layer: a quad of its tileset, sorted
    /// with the y-sorted sprites of its z by its cell (`foot_y`).
    Tile {
        z: i32,
        /// Its sorting group (`ecs::sorting_group`).
        g: crate::ecs::sorting_group::SortGroup,
        tex: &'a str,
        center: Vec2,
        size: Vec2,
        uv: [f32; 4],
        y_key: i32,
        blend: crate::renderer::material::BlendMode,
        cutout: bool,
    },
    /// A `SkinnedMesh`, skinned for this frame - sorted with everything
    /// else, where it used to be a pass of its own after every sprite, so
    /// a creature drew over the ground it walked on whatever its z.
    Skinned {
        z: i32,
        /// Its sorting group (`ecs::sorting_group`).
        g: crate::ecs::sorting_group::SortGroup,
        tex: &'a str,
        /// A material to draw through - a canvas group's picture
        /// (`MaterialComponent` on the group's entity), a tilted sprite's
        /// own.
        mat: Option<&'a str>,
        params: Params,
        tris: SkinnedTriangles,
        blend: crate::renderer::material::BlendMode,
        cutout: bool,
        /// Its place among the items of its z: a spline shape's edge (1)
        /// draws over its fill (0).
        order: i32,
        /// The `sprite_mask` it is clipped by (`MaskTable` index; 0 none):
        /// what is drawn *for* a masked sprite - its outline, its snow -
        /// keeps the sprite's clip, or an outline rings legs hidden behind
        /// cover (Shooter Classics' Crossfire).
        mask: u32,
    },
}

/// A material instance's parameters as bits (`renderer::material_params`).
type ParamBits = [u32; 16];

/// An item's material parameters: `None` for none (all zero - nearly every
/// sprite), boxed otherwise, so the 64 bytes are not in every item of the
/// pass (see [`InterleavedItem`]'s size test).
type Params = Option<Box<ParamBits>>;

fn boxed_params(p: ParamBits) -> Params {
    (p != [0; 16]).then(|| Box::new(p))
}

/// How one layer of a text is drawn (`TextOutline`, `TextShadow`).
#[derive(Clone, Copy, Default)]
enum TextStyle {
    /// As the font draws it.
    #[default]
    Plain,
    /// A distance field with its edge moved out by `grow` and softened by
    /// `soften`, in the field's units.
    Field { grow: f32, soften: f32 },
    /// A bitmap font's outline: eight copies `width` world units round it.
    Copies { width: f32 },
}

impl TextStyle {
    fn params(self) -> ParamBits {
        let mut out = [0u32; 16];
        if let TextStyle::Field { grow, soften } = self {
            out[0] = grow.to_bits();
            out[1] = soften.to_bits();
        }
        out
    }
}

/// An outline's or shadow's colour faded with its text's own alpha: fading
/// a text out used to leave its outline and shadow behind at full strength.
fn faded(color: [f32; 4], alpha: f32) -> Vec4 {
    let mut c = Vec4::from(color);
    c.w *= alpha.clamp(0.0, 1.0);
    c
}

/// World units at a text's scale, in a distance field's units: the field
/// changes by `1 / (2 * spread)` an atlas pixel, and there are `atlas_scale`
/// atlas pixels to a layout pixel. Kept under the edge, so a too-wide
/// outline is cut to what the field holds rather than turning into a box.
fn field_units(font: &FontData, scale: f32, world: f32) -> f32 {
    let spread = if font.sdf_spread > 0.0 { font.sdf_spread } else { crate::renderer::text::SDF_SPREAD };
    let layout = world / scale.max(1e-4);
    (layout * font.atlas_scale / (2.0 * spread)).clamp(0.0, 0.45)
}

/// How a sprite (or mesh) with this material blends: the component's
/// `blend_mode` override, else a built-in material named for its blend
/// (`"additive"`, `"multiply"`, `"screen"`), else alpha.
///
/// Until Sep 2026 no renderer read this at all. `blend_mode` had a field, an
/// inspector, a doc comment promising it "overrides the material's built-in
/// blend mode", and test-lab scene 02 labelled "Alpha | Additive | Multiply" -
/// and every one of them drew alpha-blended, in every runtime. A graph
/// material's pipeline is built with its own `.material` blend and does not
/// consult this.
pub fn sprite_blend(m: Option<&MaterialComponent>) -> crate::renderer::material::BlendMode {
    use crate::renderer::material::BlendMode;
    let Some(m) = m else { return BlendMode::Alpha };
    if let Some(b) = m.blend_mode {
        return b;
    }
    match m.material_id.as_str() {
        "additive" => BlendMode::Additive,
        "multiply" => BlendMode::Multiply,
        "screen" => BlendMode::Screen,
        _ => BlendMode::Alpha,
    }
}

/// A sort rank for a blend mode, so sprites that share a texture and a
/// blend sit next to each other and batch. `BlendMode` is not `Ord`.
fn blend_rank(b: crate::renderer::material::BlendMode) -> u8 {
    use crate::renderer::material::BlendMode;
    match b {
        BlendMode::Alpha => 0,
        BlendMode::Additive => 1,
        BlendMode::Multiply => 2,
        BlendMode::Screen => 3,
        BlendMode::PremultipliedAlpha => 4,
    }
}

fn material_param_bits(m: Option<&MaterialComponent>) -> ParamBits {
    match m {
        // The entity's own values, or its material file's defaults.
        Some(m) if !m.uniforms.is_empty() || crate::renderer::material_params::has_defaults(&m.material_id) => {
            crate::renderer::material_params::bits(
                &crate::renderer::material_params::packed_for(&m.material_id, &m.uniforms),
            )
        }
        _ => [0; 16],
    }
}

/// The frame's masks (`renderer::mask`): every `sprite_mask` by its entity's
/// name, and the uniforms the masked things draw with, deduplicated, so a
/// draw item carries an index and equal masks still batch. Index 0 is none.
pub struct MaskTable {
    shapes: std::collections::HashMap<String, (crate::ecs::components::SpriteMask, WorldTransform)>,
    uniforms: Vec<crate::renderer::mask::MaskUniform>,
    /// A texture mask's texture id, parallel to `uniforms`.
    textures: Vec<Option<String>>,
}

impl MaskTable {
    /// Every mask in `world`, placed by its world transform. Cheap when
    /// there are none: one empty query.
    pub fn gather(world: &World) -> Self {
        let mut shapes = std::collections::HashMap::new();
        for (_, (m, n, t, w)) in world
            .query::<(&crate::ecs::components::SpriteMask, &crate::ecs::components::Name, &Transform, Option<&WorldTransform>)>()
            .iter()
        {
            shapes.insert(n.0.clone(), (m.clone(), w.cloned().unwrap_or_else(|| WorldTransform::from(t))));
        }
        Self { shapes, uniforms: vec![crate::renderer::mask::MaskUniform::NONE], textures: vec![None] }
    }

    /// The index of `masked`'s mask, packing it on first sight; 0 when it
    /// names nothing (or nothing is masked).
    pub fn index(&mut self, masked: Option<&crate::ecs::components::Masked>) -> u32 {
        let Some(m) = masked else { return 0 };
        let Some((mask, at)) = self.shapes.get(&m.by) else { return 0 };
        let u = crate::renderer::mask::pack(&mask.shape, at, m.outside, mask.feather);
        let texture = mask.shape.texture_id().map(str::to_string);
        if let Some(i) = (0..self.uniforms.len()).find(|&i| self.uniforms[i] == u && self.textures[i] == texture) {
            return i as u32;
        }
        self.uniforms.push(u);
        self.textures.push(texture);
        (self.uniforms.len() - 1) as u32
    }

    /// The index of a mask made on the spot - a silhouette's occluder
    /// (`ecs::silhouette`) - packed and deduplicated as a named one is.
    pub fn index_shape(&mut self, shape: &crate::renderer::mask::MaskShape, at: &WorldTransform) -> u32 {
        let u = crate::renderer::mask::pack(shape, at, false, 0.0);
        let texture = shape.texture_id().map(str::to_string);
        if let Some(i) = (0..self.uniforms.len()).find(|&i| self.uniforms[i] == u && self.textures[i] == texture) {
            return i as u32;
        }
        self.uniforms.push(u);
        self.textures.push(texture);
        (self.uniforms.len() - 1) as u32
    }

    pub fn uniform(&self, index: u32) -> crate::renderer::mask::MaskUniform {
        self.uniforms.get(index as usize).copied().unwrap_or(crate::renderer::mask::MaskUniform::NONE)
    }

    /// The texture a texture mask reads (`MaskShape::Texture`), if `index`
    /// is one.
    pub fn texture(&self, index: u32) -> Option<&str> {
        self.textures.get(index as usize).and_then(|t| t.as_deref())
    }

    /// Set mask `index` on `batch` for the next flushes - and, for a texture
    /// mask, hand the batch its texture through `flush`, the host's own
    /// callback, so the host uploads and finds it as it would a sprite's
    /// (`WgpuSpriteBatch::request_mask_texture`).
    pub fn apply(&self, index: u32, batch: &mut WgpuSpriteBatch, flush: &mut impl FnMut(&mut WgpuSpriteBatch, &str)) {
        batch.set_mask(self.uniform(index));
        if let Some(texture) = self.texture(index) {
            batch.request_mask_texture();
            flush(batch, texture);
            batch.end_mask_texture_request();
        }
    }
}

impl InterleavedItem<'_> {
    /// The mask this item draws with, in the frame's [`MaskTable`].
    fn mask(&self) -> u32 {
        match self {
            InterleavedItem::Sprite { mask, .. }
            | InterleavedItem::Mesh { mask, .. }
            | InterleavedItem::Path { mask, .. }
            | InterleavedItem::Text { mask, .. }
            | InterleavedItem::Icon { mask, .. }
            | InterleavedItem::Skinned { mask, .. } => *mask,
            _ => 0,
        }
    }

    /// The parameters this item draws with - zero for anything but a sprite
    /// or mesh with a material that takes them.
    /// The parameters, or `None` when they are all zero - which is nearly
    /// every item, and costs no 64-byte copy to say.
    fn params_if_any(&self) -> Option<ParamBits> {
        match self {
            InterleavedItem::Sprite { params, .. }
            | InterleavedItem::Mesh { params, .. }
            | InterleavedItem::Skinned { params, .. } => params.as_deref().copied(),
            InterleavedItem::Text { style, .. } => Some(style.params()).filter(|p| *p != [0; 16]),
            _ => None,
        }
    }

    fn params(&self) -> ParamBits {
        match self {
            InterleavedItem::Sprite { params, .. }
            | InterleavedItem::Mesh { params, .. }
            | InterleavedItem::Skinned { params, .. } => params.as_deref().copied().unwrap_or([0; 16]),
            InterleavedItem::Text { style, .. } => style.params(),
            _ => [0; 16],
        }
    }

    /// How this item blends: a sprite's or mesh's material decides; text,
    /// paths and skinned meshes are alpha. (Shadows flush through their own
    /// multiply pipeline and never read it.)
    fn blend(&self) -> crate::renderer::material::BlendMode {
        match self {
            InterleavedItem::Sprite { blend, .. }
            | InterleavedItem::Mesh { blend, .. }
            | InterleavedItem::Text { blend, .. }
            | InterleavedItem::Path { blend, .. }
            | InterleavedItem::Skinned { blend, .. }
            | InterleavedItem::Icon { blend, .. }
            | InterleavedItem::Tile { blend, .. }
            | InterleavedItem::Gpu { blend, .. } => *blend,
            InterleavedItem::Shadow { .. } => crate::renderer::material::BlendMode::Alpha,
        }
    }

    /// Draw as a cutout or not, in `blend` - a layer's decision. `false` for
    /// an item a cutout leaves out entirely (a GPU particle emitter).
    fn set_layer(&mut self, cut: bool, how: crate::renderer::material::BlendMode) -> bool {
        match self {
            InterleavedItem::Sprite { cutout, blend, .. }
            | InterleavedItem::Mesh { cutout, blend, .. }
            | InterleavedItem::Text { cutout, blend, .. }
            | InterleavedItem::Path { cutout, blend, .. }
            | InterleavedItem::Icon { cutout, blend, .. }
            | InterleavedItem::Tile { cutout, blend, .. }
            | InterleavedItem::Skinned { cutout, blend, .. } => {
                *cutout = cut;
                *blend = how;
                true
            }
            InterleavedItem::Gpu { blend, .. } => {
                *blend = how;
                !cut
            }
            InterleavedItem::Shadow { .. } => true,
        }
    }

    /// Whether this item is drawn as a cutout ([`WorldLayer`]).
    fn is_cutout(&self) -> bool {
        match self {
            InterleavedItem::Sprite { cutout, .. }
            | InterleavedItem::Mesh { cutout, .. }
            | InterleavedItem::Text { cutout, .. }
            | InterleavedItem::Path { cutout, .. }
            | InterleavedItem::Icon { cutout, .. }
            | InterleavedItem::Tile { cutout, .. }
            | InterleavedItem::Skinned { cutout, .. } => *cutout,
            InterleavedItem::Shadow { .. } | InterleavedItem::Gpu { .. } => false,
        }
    }

    /// The colour multiplier this item is queued with.
    fn mul(&self) -> Vec4 {
        match self {
            InterleavedItem::Sprite { cutout, .. }
            | InterleavedItem::Mesh { cutout, .. }
            | InterleavedItem::Text { cutout, .. }
            | InterleavedItem::Path { cutout, .. }
            | InterleavedItem::Icon { cutout, .. }
            | InterleavedItem::Tile { cutout, .. }
            | InterleavedItem::Skinned { cutout, .. } if *cutout => CUTOUT,
            _ => Vec4::ONE,
        }
    }

    /// Extract the bucket key used for sorting & grouping. Tuple layout:
    /// `(z_order, kind: 0 = shadow / 1 = sprite / 2 = text, texture_id,
    /// material_id)`.
    ///
    /// At equal `z_order` shadows MUST sort before sprites so a sprite at
    /// that same z paints on top of its shadow rather than under it (this is
    /// the whole point of interleaving — pre-Phase 3.c-4 polish the shadow
    /// pass was a single monolithic batch that always preceded every sprite,
    /// so any sprite with `z < shadow.z` (typically a backdrop) covered the
    /// shadow entirely). Text sorts after sprites of the same z, which is
    /// the order the separate text pass gave within a band.
    ///
    /// The third field is the sprite's `y_sort` key: within a z band and
    /// kind, a sprite that opted in draws by how low it stands, before
    /// texture grouping. This pass - the one every runtime draws its world
    /// with - lost that when it became the main path, so `y_sort` did
    /// nothing anywhere while the older `render_sprites_wgpu_multi`, which
    /// its pixel test rendered through, still honoured it (BUGS #162).
    ///
    /// The key is `(outer, own)`: `own` is the tuple above, and `outer` is
    /// the same for a drawable in no sorting group - so such a frame sorts
    /// exactly as it always did - and for one in a group, the group's place
    /// as a sprite at its z and y (`ecs::sorting_group::outer_key`), which
    /// keeps the group's members together and in their own order.
    fn sort_key(&self) -> ((i32, u8, i32, u32), (i32, u8, i32, TexKey<'_>, Option<TexKey<'_>>)) {
        let own = self.own_key();
        (crate::ecs::sorting_group::outer_key((own.0, own.1, own.2), self.group()), own)
    }

    /// This drawable's sorting group.
    fn group(&self) -> crate::ecs::sorting_group::SortGroup {
        match self {
            InterleavedItem::Shadow { g, .. }
            | InterleavedItem::Sprite { g, .. }
            | InterleavedItem::Text { g, .. }
            | InterleavedItem::Icon { g, .. }
            | InterleavedItem::Mesh { g, .. }
            | InterleavedItem::Path { g, .. }
            | InterleavedItem::Gpu { g, .. }
            | InterleavedItem::Tile { g, .. }
            | InterleavedItem::Skinned { g, .. } => *g,
        }
    }

    /// The key a drawable has of its own, its group aside.
    fn own_key(&self) -> (i32, u8, i32, TexKey<'_>, Option<TexKey<'_>>) {
        let (z, kind, sub, tex, mat): (i32, u8, i32, &str, Option<&str>) = match self {
            InterleavedItem::Shadow { z, tex, .. } => (*z, 0, 0, tex, None),
            InterleavedItem::Sprite { z, tex, mat, transform, sprite, .. } => {
                (*z, 1, sprite.y_sort_key(transform.position.y), tex, *mat)
            }
            InterleavedItem::Text { z, tex, layer, .. } => (*z, 2, *layer as i32, tex, None),
            InterleavedItem::Mesh { z, tex, mat, .. } => (*z, 1, 0, tex, *mat),
            InterleavedItem::Tile { z, tex, y_key, .. } => (*z, 1, *y_key, tex, None),
            InterleavedItem::Path { z, .. } => (*z, 1, 0, crate::asset::WHITE_TEXTURE_ID, None),
            InterleavedItem::Skinned { z, tex, order, mat, .. } => (*z, 1, *order, tex, *mat),
            InterleavedItem::Icon { z, tex, .. } => (*z, 2, 0, tex, None),
            InterleavedItem::Gpu { z, tex, .. } => (*z, 3, 0, tex, None),
        };
        (z, kind, sub, TexKey(tex), mat.map(TexKey))
    }
}

/// What each layer of a lit frame makes of one collected item
/// ([`render_world_items`]): what [`layer_draw`] needs, and the two things that
/// leave an item out of a layer whatever its blend - a material that reads
/// the scene (drawn only in the readers' pass, `WorldLayer::All`), and a
/// shadow (no part of the emission layer).
#[derive(Clone, Copy, Debug)]
struct LayerRule {
    unlit: bool,
    base: crate::renderer::material::BlendMode,
    reads_scene: bool,
    shadow: bool,
}

impl LayerRule {
    fn new(unlit: bool, base: crate::renderer::material::BlendMode) -> Self {
        LayerRule { unlit, base, reads_scene: false, shadow: false }
    }

    fn shadow() -> Self {
        LayerRule { shadow: true, ..Self::new(false, crate::renderer::material::BlendMode::Alpha) }
    }

    fn reading_scene(mut self, material: Option<&str>) -> Self {
        self.reads_scene = material.is_some_and(crate::renderer::material_params::reads_scene);
        self
    }

    /// Whether `item` draws in `layer`, setting its cutout and blend for it
    /// if so - as the collection used to decide per call.
    fn admits(&self, layer: WorldLayer, item: &mut InterleavedItem<'_>) -> bool {
        if self.shadow {
            return layer != WorldLayer::Emission;
        }
        if self.reads_scene && layer != WorldLayer::All {
            return false;
        }
        let Some((cutout, blend)) = layer_draw(layer, self.unlit, self.base) else { return false };
        item.set_layer(cutout, blend)
    }
}

/// An item's whole place in the draw order, as integers: the order
/// [`InterleavedItem::sort_key`] then params, blend and mask give, with every
/// string and parameter block replaced by its rank among this frame's (see
/// [`item_keys`]). Built once per item, where the comparator used to rebuild
/// both items' keys on every comparison and, on the ties ten thousand
/// identical lanterns are, go on to compare their 64-byte parameter blocks.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct ItemKey {
    outer: (i32, u8, i32, u32),
    z: i32,
    kind: u8,
    sub: i32,
    tex: u32,
    /// 0 for no material, else the material's rank + 1, as `None` sorts
    /// before `Some`.
    mat: u32,
    params: u32,
    blend: u8,
    mask: u32,
}

impl ItemKey {
    /// What a batch is: two items with equal batch fields (and cutout) draw
    /// in one flush.
    fn batch(&self) -> (u8, u32, u32, u32, u8, u32) {
        (self.kind, self.tex, self.mat, self.params, self.blend, self.mask)
    }
}

/// Every item's [`ItemKey`], each beside its index in `items`. Textures and
/// materials are ranked by their text and parameter blocks by their bits,
/// among the few distinct ones a frame has; a run of one (the common case)
/// costs a pointer compare per item, and nothing is hashed.
fn item_keys(items: &[InterleavedItem], active: &[u32]) -> Vec<(ItemKey, u32)> {
    /// The distinct values met so far, with the last one remembered.
    struct Distinct<T> {
        seen: Vec<T>,
        last: usize,
    }
    impl<T> Distinct<T> {
        fn index(&mut self, v: T, same: impl Fn(&T, &T) -> bool) -> u32 {
            if let Some(l) = self.seen.get(self.last)
                && same(l, &v)
            {
                return self.last as u32;
            }
            let at = match self.seen.iter().position(|s| same(s, &v)) {
                Some(at) => at,
                None => {
                    self.seen.push(v);
                    self.seen.len() - 1
                }
            };
            self.last = at;
            at as u32
        }
        /// Each distinct value's rank by `order`; equal values share one.
        fn ranks(&self, order: impl Fn(&T, &T) -> std::cmp::Ordering) -> Vec<u32> {
            let mut by: Vec<usize> = (0..self.seen.len()).collect();
            by.sort_by(|&a, &b| order(&self.seen[a], &self.seen[b]));
            let mut ranks = vec![0u32; self.seen.len()];
            let mut rank = 0u32;
            for (n, &at) in by.iter().enumerate() {
                if n > 0 && order(&self.seen[by[n - 1]], &self.seen[at]) != std::cmp::Ordering::Equal {
                    rank += 1;
                }
                ranks[at] = rank;
            }
            ranks
        }
    }
    let same_ptr = |a: &&str, b: &&str| a.as_ptr() == b.as_ptr() && a.len() == b.len();
    let mut texs: Distinct<&str> = Distinct { seen: Vec::new(), last: 0 };
    let mut mats: Distinct<&str> = Distinct { seen: Vec::new(), last: 0 };
    let mut blocks: Distinct<ParamBits> = Distinct { seen: Vec::new(), last: 0 };
    let mut keys: Vec<(ItemKey, u32)> = Vec::with_capacity(active.len());
    for &n in active {
        let it = &items[n as usize];
        // `sort_key` is the order's definition; this replaces its strings
        // (and the parameters, blend and mask after it) by integers.
        let (outer, (z, kind, sub, tex, mat)) = it.sort_key();
        let params = it.params_if_any();
        keys.push((
            ItemKey {
                outer,
                z,
                kind,
                sub,
                tex: texs.index(tex.0, same_ptr),
                mat: mat.map_or(0, |m| mats.index(m.0, same_ptr) + 1),
                // 0 is no parameters - all zero, the smallest block there is.
                params: params.map_or(0, |p| blocks.index(p, |a, b| a == b) + 1),
                blend: blend_rank(it.blend()),
                mask: it.mask(),
            },
            n,
        ));
    }
    let tex_rank = texs.ranks(|a, b| a.cmp(b));
    let mat_rank = mats.ranks(|a, b| a.cmp(b));
    let block_rank = blocks.ranks(|a, b| a.cmp(b));
    for (k, _) in &mut keys {
        k.tex = tex_rank[k.tex as usize];
        if k.mat > 0 {
            k.mat = mat_rank[k.mat as usize - 1] + 1;
        }
        if k.params > 0 {
            k.params = block_rank[k.params as usize - 1] + 1;
        }
    }
    keys
}

/// A texture or material id in a sort key: ordered as the string orders, but
/// two ids sharing one string - every interned `TextureId` - are equal by
/// pointer, without `memcmp`. Ten thousand lanterns of one texture used to
/// compare their paths byte by byte at every step of the sort
/// (`docs/architecture-debt.md`, #1).
#[derive(Clone, Copy, Debug)]
pub struct TexKey<'a>(pub &'a str);

impl PartialEq for TexKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        crate::asset::asset_id::same_str(self.0, other.0)
    }
}
impl Eq for TexKey<'_> {}
impl PartialOrd for TexKey<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for TexKey<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        crate::asset::asset_id::cmp_str(self.0, other.0)
    }
}

/// Pipeline-selection tag passed to the unified flush callback used by
/// [`render_sprites_with_shadows_wgpu_materialized`]. Lets a single
/// `FnMut` closure cover both the normal alpha-blend sprite path and the
/// multiply-blend shadow path without forcing two simultaneous mutable
/// captures of `assets` / `pass` / etc. at the runtime call site.
#[derive(Debug, Clone, Copy)]
pub enum SpriteShadowFlush<'a> {
    /// Regular sprite quad batch. `material_id` is the entity's
    /// `MaterialComponent.material_id` (or `None` if no material). Flush
    /// via `batch.flush(...)` or `batch.flush_with_pipeline(...)`.
    Sprite { material_id: Option<&'a str> },
    /// Shadow quad batch. Flush via `batch.flush_multiply(...)` so the
    /// coverage-weighted multiply blend darkens whatever is beneath.
    Shadow,
}

/// Which part of a frame an interleaved world pass draws - how `Unlit` things
/// are sorted **with** lit ones.
///
/// `Unlit` used to be a separate pass after the lighting composite, so an
/// unlit sprite drew over every lit one whatever the z orders said: a lit
/// window on a far tower showed through the near building in front of it.
/// Now one z-sorted pass draws the world twice when a lit scene has unlit
/// things in it:
///
/// - [`WorldLayer::Scene`] into the scene buffer the lights multiply: lit
///   things as themselves, and each unlit thing as a **cutout** - its shape
///   in black at its alpha - so what it covers goes dark (an additive one
///   covers nothing and is skipped).
/// - [`WorldLayer::Emission`] into the emission buffer the composite adds
///   unlit: unlit things as themselves, and each lit alpha-blended thing as a
///   cutout, so an unlit sprite behind a lit one is hidden by it.
///
/// The composite is `scene * light + emission`. [`WorldLayer::All`] is a
/// frame with no lighting pass, where everything draws as itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldLayer {
    All,
    Scene,
    Emission,
}

/// A colour multiplier that draws a drawable's shape in black at its own
/// alpha - a cutout.
const CUTOUT: Vec4 = Vec4::new(0.0, 0.0, 0.0, 1.0);

/// How a drawable appears in `layer`: `None` to skip it, else whether it is a
/// cutout and the blend it draws with.
fn layer_draw(
    layer: WorldLayer,
    unlit: bool,
    blend: crate::renderer::material::BlendMode,
) -> Option<(bool, crate::renderer::material::BlendMode)> {
    use crate::renderer::material::BlendMode;
    match (layer, unlit) {
        (WorldLayer::All, _) | (WorldLayer::Scene, false) => Some((false, blend)),
        // Multiply adds nothing: an unlit multiply darkens the scene like a
        // lit one...
        (WorldLayer::Scene, true) => match blend {
            BlendMode::Additive | BlendMode::Screen => None,
            BlendMode::Multiply => Some((false, blend)),
            _ => Some((true, BlendMode::Alpha)),
        },
        // ...and a multiply, lit or not, darkens the emission behind it too.
        // It used to be skipped here, so dust drawn over a glowing nebula
        // darkened only the lit scene under the glow and left the glow as it
        // was - the showcase's Nebula met it.
        (WorldLayer::Emission, true) => Some((false, blend)),
        (WorldLayer::Emission, false) => match blend {
            BlendMode::Alpha | BlendMode::PremultipliedAlpha => Some((true, BlendMode::Alpha)),
            BlendMode::Multiply => Some((false, blend)),
            _ => None,
        },
    }
}

/// Whether anything in the world draws with a graph that reads the scene -
/// whether a host needs the scene pass after its world pass. A host used to
/// learn it from its own world flush skipping such a sprite, but a lit frame
/// leaves them out before the flush ([`leaves_out`]), so in every lit scene
/// the scene pass never ran and Scene Color sprites drew nothing. Known once
/// the material's graph has compiled (`material_params::reads_scene`).
pub fn has_scene_readers(world: &World) -> bool {
    world
        .query::<&MaterialComponent>()
        .iter()
        .any(|(_, m)| crate::renderer::material_params::reads_scene(&m.material_id))
}

/// Which part of the world, by resolved z, the world passes draw - how a
/// host draws the world in two parts around its Scene Color sprites.
///
/// A sprite whose graph reads the scene (`SceneColor`) draws over a copy of
/// what is already drawn, so the copy has to be taken before anything meant
/// to stand in front of it. Until Sep 2026 every such sprite drew after the
/// whole world, over every other sprite whatever its z - a boat on a
/// reflecting lake sank under the water. A host now draws the world
/// [`ZWindow::UpTo`] the lowest reader's z, takes the copy, draws the
/// readers, then draws the rest [`ZWindow::Above`] it - which is Godot's
/// back-buffer copy. Since Oct 2026 it does that at **every** z a reader
/// sits at ([`SceneReaderPlan`]), so a reader sees the readers below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZWindow {
    /// Resolved z at most this.
    UpTo(i32),
    /// Resolved z above this.
    Above(i32),
    /// Resolved z above the first and at most the second.
    Between(i32, i32),
    /// Resolved z exactly this - one band of readers.
    Band(i32),
}

impl ZWindow {
    pub fn contains(self, z: i32) -> bool {
        match self {
            ZWindow::UpTo(t) => z <= t,
            ZWindow::Above(t) => z > t,
            ZWindow::Between(lo, hi) => z > lo && z <= hi,
            ZWindow::Band(t) => z == t,
        }
    }
}

/// How a host draws a world with Scene Color sprites in it: in **parts**,
/// cut at each z a reader sits at. After each part but the last the host
/// copies what is drawn and draws the readers at that cut's z over the copy
/// ([`Self::readers_after`]), so a reader reads everything below it -
/// readers further down included - and nothing above it.
///
/// Until Oct 2026 there was one cut, at the lowest reader: every reader read
/// one copy taken below all of them, so a heat haze over a reflecting pool
/// bent the pool's floor and not its reflection, and two refracting panes
/// could not see each other (Godot's `BackBufferCopy` had the same rule).
/// Each further cut costs a copy and, in a lit frame, a composite.
///
/// With one cut and nothing above it the plan is one part and one reader
/// pass, which is what every frame without readers in front of each other
/// did before.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SceneReaderPlan {
    /// The resolved z of every band of readers, lowest first.
    cuts: Vec<i32>,
    /// Whether anything that is not a reader draws above the highest cut.
    rest_above: bool,
}

impl SceneReaderPlan {
    pub fn of(world: &World, sort_layers: &SortLayers) -> Self {
        use crate::renderer::material_params::reads_scene;
        let mut cuts: Vec<i32> = world
            .query::<(&MaterialComponent, Option<&SpriteComponent>, Option<&Mesh2D>)>()
            .iter()
            .filter(|(_, (m, _, _))| reads_scene(&m.material_id))
            .filter_map(|(_, (_, sprite, mesh))| match (sprite, mesh) {
                (Some(s), _) => Some(sort_layers.resolve(s.sort_layer.as_deref(), s.z_order)),
                (None, Some(mesh)) => Some(sort_layers.resolve(mesh.sort_layer.as_deref(), mesh.z_order)),
                _ => None,
            })
            .collect();
        cuts.sort_unstable();
        cuts.dedup();
        let rest_above = cuts.last().is_some_and(|&t| anything_above(world, sort_layers, t));
        Self { cuts, rest_above }
    }

    /// How many parts the world is drawn in: one when there are no readers.
    pub fn parts(&self) -> usize {
        if self.cuts.is_empty() { 1 } else { self.cuts.len() + self.rest_above as usize }
    }

    /// The z window part `part` of the world is drawn with (`None`: all).
    pub fn world_window(&self, part: usize) -> Option<ZWindow> {
        let (first, last) = (*self.cuts.first()?, *self.cuts.last()?);
        Some(match part {
            0 => ZWindow::UpTo(first),
            k if k < self.cuts.len() => ZWindow::Between(self.cuts[k - 1], self.cuts[k]),
            _ => ZWindow::Above(last),
        })
    }

    /// The readers to draw over a copy after part `part`: `Some(window)`
    /// when a band of readers follows it, `None` when none does.
    pub fn readers_after(&self, part: usize) -> Option<ZWindow> {
        self.cuts.get(part).map(|&t| ZWindow::Band(t))
    }

    /// The reader bands, lowest first - for tests.
    pub fn cuts(&self) -> &[i32] {
        &self.cuts
    }
}

/// Whether anything that is not a Scene Color reader draws above `t`.
fn anything_above(world: &World, sort_layers: &SortLayers, t: i32) -> bool {
    use crate::renderer::material_params::reads_scene;
    let reader = |e: hecs::Entity| world.get::<&MaterialComponent>(e).is_ok_and(|m| reads_scene(&m.material_id));
    let above = |layer: Option<&str>, z: i32| sort_layers.resolve(layer, z) > t;
    world.query::<&SpriteComponent>().iter().any(|(e, s)| above(s.sort_layer.as_deref(), s.z_order) && !reader(e))
        || world.query::<&Mesh2D>().iter().any(|(e, m)| above(m.sort_layer.as_deref(), m.z_order) && !reader(e))
        || world.query::<&TextComponent>().iter().any(|(_, x)| above(x.sort_layer.as_deref(), x.z_order))
        || world.query::<&VectorPath>().iter().any(|(_, x)| above(x.sort_layer.as_deref(), x.z_order))
        || world.query::<&crate::ecs::components::Trail>().iter().any(|(_, x)| above(x.sort_layer.as_deref(), x.z_order))
        || world.query::<&ParticleEmitter>().iter().any(|(_, x)| above(x.sort_layer.as_deref(), x.z_order))
        || world.query::<&Tilemap>().iter().any(|(_, x)| above(x.sort_layer.as_deref(), x.z_order))
        || skinned_above(world, &above)
}

/// Whether a skinned mesh sorts above the window's floor (`ecs::frame_draw`'s
/// scene readers) - none in a build without skinned meshes.
#[cfg(feature = "skinned_mesh")]
fn skinned_above(world: &World, above: &impl Fn(Option<&str>, i32) -> bool) -> bool {
    world.query::<&SkinnedMesh>().iter().any(|(_, x)| above(None, x.z_order))
}

#[cfg(not(feature = "skinned_mesh"))]
fn skinned_above(_: &World, _: &impl Fn(Option<&str>, i32) -> bool) -> bool {
    false
}

thread_local! {
    static Z_WINDOW: std::cell::Cell<Option<ZWindow>> = const { std::cell::Cell::new(None) };
}

/// Draw only the part of the world in `window` (`None`: all of it) in the
/// world passes that follow - sprites, text, meshes, paths, trails,
/// shadows, tilemaps, backdrops and particles. Like the frame view, a host
/// sets it per pass and clears it after; the normal pass ignores it, since
/// the lights are drawn once for both parts.
pub fn set_z_window(window: Option<ZWindow>) {
    Z_WINDOW.with(|w| w.set(window));
}

/// Whether a drawable at resolved z `z` is drawn by the current pass.
pub fn in_z_window(z: i32) -> bool {
    Z_WINDOW.with(|w| w.get().is_none_or(|w| w.contains(z)))
}

/// Where a host would split its world around the Scene Color sprites if
/// there were one cut: the lowest reader's z, when something that is not a
/// reader draws above it. Hosts use [`SceneReaderPlan`], which cuts at every
/// reader band.
pub fn scene_reader_split(world: &World, sort_layers: &SortLayers) -> Option<i32> {
    let plan = SceneReaderPlan::of(world, sort_layers);
    let t = *plan.cuts.first()?;
    anything_above(world, sort_layers, t).then_some(t)
}

/// Render `ShadowCaster` + regular `SpriteComponent` entities in a single
/// pass, interleaved by `z_order` so shadows fall onto whatever sprite is
/// directly beneath them in z (a backdrop, a tilemap, another sprite).
///
/// **Why this exists vs. the old `render_shadows_wgpu` + `render_sprites_wgpu_multi_materialized`
/// pair:** the old path drew _all_ shadows before _any_ sprite, regardless
/// of z. With no depth test (the wgpu sprite pipeline has none), this meant
/// any sprite with `z_order < shadow.z_order` — most notably backdrops —
/// would paint over the shadow and hide it entirely. Interleaving fixes
/// that without needing a depth buffer or a per-z render-target sandwich.
///
/// **Ordering rules:**
/// 1. Items sort by `z_order` ascending (back to front).
/// 2. At equal `z_order`, shadow draws first so the matching caster sprite
///    layers on top of its own shadow.
/// 3. Within each `(kind, z, tex, mat)` bucket, items batch into one draw
///    call via `flush` with the matching [`SpriteShadowFlush`] tag.
///
/// **Lit sprites** (entities with `MaterialComponent.material_id == "lit_sprite"`)
/// are skipped here — they still route through the dedicated lit pipeline
/// via [`render_lit_sprites_wgpu`]. Shadows for lit-sprite entities are
/// emitted normally (they only need the sprite's base color/texture, not
/// the lit pipeline).
///
/// **Single-closure API:** the runtime needs `&mut` access to its asset
/// cache, warned-set and render pass inside the flush body. Two separate
/// `FnMut` closures (one for sprites, one for shadows) would force two
/// simultaneous mutable captures and trip the borrow checker. Routing
/// both through one closure with a [`SpriteShadowFlush`] tag sidesteps
/// the problem cleanly.
///
/// Returns `(sprite_count, shadow_count)` of the quads queued (multi-quad
/// `SpriteFill` modes count as 1 here, matching the pre-existing sprite
/// system's bookkeeping; a text counts as 1 sprite).
///
/// **World text is drawn here too**, in z order with the sprites, with the
/// font atlas as its texture group and `fonts` resolving each `font_id`.
/// Text used to be its own pass after every sprite pass, so a label at z 5
/// drew over a sprite at z 50; now `z_order` on text means what it means on
/// a sprite. `Unlit` text stays with the unlit sprites, drawn after the
/// lighting composite by the emission layer ([`WorldLayer`]).
pub fn render_sprites_with_shadows_wgpu_materialized(
    world: &World,
    sort_layers: &SortLayers,
    batch: &mut WgpuSpriteBatch,
    fonts: &FrameFonts<'_>,
    default_size: Vec2,
    layer: WorldLayer,
    flush: impl FnMut(&mut WgpuSpriteBatch, &str, SpriteShadowFlush<'_>),
) -> (usize, usize) {
    render_world_items(world, sort_layers, fonts, default_size, |w| w.draw(layer, batch, flush))
}

crate::feature_component!(type JellyC = "jelly" => crate::ecs::jelly::Jelly);
use crate::ecs::carve::{PaintableC, painted_texture};
crate::feature_component!(type StageBillboardC = "stage" => crate::ecs::stage::StageBillboard);
crate::feature_component!(type StageQuadC = "stage" => crate::ecs::stage::StageQuad);
crate::feature_component!(type SpriteTiltC = "sprite_tilt" => crate::ecs::sprite_tilt::SpriteTilt);
crate::feature_component!(type SpriteOutlineC = "sprite_outline" => crate::ecs::sprite_outline::SpriteOutline);
crate::feature_component!(type SnowCoverC = "snow" => crate::ecs::snow::SnowCover);

/// Collect the world's drawables once - query, cull, build each item, note
/// what each layer will make of it - and hand them to `f`, which draws them
/// with [`WorldItems::draw`] as many times as the frame needs: a lit frame's
/// scene layer, its emission layer and its scene-reader bands are all one
/// collection now (architecture-debt #9). Items borrow the world's
/// components, which is why this takes a closure rather than returning them.
/// What the frame view culls is decided here, so collect per camera.
pub fn render_world_items<R>(
    world: &World,
    sort_layers: &SortLayers,
    fonts: &FrameFonts<'_>,
    default_size: Vec2,
    f: impl FnOnce(&mut WorldItems<'_>) -> R,
) -> R {
    let canvas_ids: Vec<String> = world
        .query::<&crate::ecs::canvas_group::CanvasGroup>()
        .iter()
        .map(|(g, _)| crate::ecs::canvas_group::target_id(g))
        .collect();
    let mut canvas_targets: Vec<(hecs::Entity, i32, bool, crate::renderer::material::BlendMode, [f32; 4], [Vec2; 4])> =
        Vec::new();
    // Falling sand's glow (`ecs::pixel_sim`): fire and lava again, additive.
    #[cfg(feature = "pixel_sim")]
    let pixel_glows: Vec<(hecs::Entity, [Vec2; 4], f32, i32, String)> = crate::ecs::pixel_sim::glow_quads(world)
        .into_iter()
        .map(|(e, c, k, z)| (e, c, k, z, crate::ecs::pixel_sim::glow_id(e)))
        .collect();
    // Mirrors' textures (`ecs::mirror`), owned here for the same reason.
    #[cfg(feature = "mirror")]
    let mirror_ids: Vec<(hecs::Entity, String)> = world
        .query::<&crate::ecs::mirror::Mirror>()
        .iter()
        .filter(|(m, _)| crate::ecs::mirror::is_active(*m))
        .map(|(m, _)| (m, crate::ecs::mirror::target_id(m)))
        .collect();
    // Portals' textures (`ecs::portal`), likewise.
    #[cfg(feature = "portal")]
    let portal_ids: Vec<(hecs::Entity, String)> = world
        .query::<&crate::ecs::portal::Portal>()
        .iter()
        .filter(|(p, _)| crate::ecs::portal::is_active(*p))
        .map(|(p, _)| (p, crate::ecs::portal::target_id(p)))
        .collect();
    // A group's `threshold` (a surface from soft blobs): the edge and its
    // softness in `params[1]`, which the default sprite shader reads.
    let canvas_thresholds: Vec<(hecs::Entity, f32, f32)> = world
        .query::<&crate::ecs::canvas_group::CanvasGroup>()
        .iter()
        .filter(|(_, g)| g.threshold > 0.0)
        .map(|(e, g)| (e, g.threshold, g.threshold_soft))
        .collect();
    // A canvas group's material (`MaterialComponent` on the group's entity):
    // its whole picture is drawn through it - an outline round the group, a
    // shimmer over it. Owned here, before `items`, which borrows the ids. The
    // picture is premultiplied, so that is the blend unless the material
    // names another.
    let canvas_mats: Vec<(hecs::Entity, crate::asset::MaterialId, ParamBits, crate::renderer::material::BlendMode)> = world
        .query::<(&crate::ecs::canvas_group::CanvasGroup, &MaterialComponent)>()
        .iter()
        .map(|(g, (_, m))| {
            let blend = m.blend_mode.unwrap_or(crate::renderer::material::BlendMode::PremultipliedAlpha);
            (g, m.material_id.clone(), material_param_bits(Some(m)), blend)
        })
        .collect();
    // Room for every sprite up front (hecs knows the count without walking
    // them): grown from empty, ten thousand items were regrown and copied
    // about fourteen times a pass, 11% of a frame of Ten Thousand Lights.
    let mut items: Vec<InterleavedItem> =
        Vec::with_capacity(world.query::<&SpriteComponent>().iter().len() + 64);
    let ceiling = backdrop_ceiling(world, sort_layers);
    let view = frame_view();
    let mut masks = MaskTable::gather(world);
    // What each layer makes of each item: `rules[i]` for `items[i]`. A rule
    // is noted where an entity's is known and pads the list with the
    // previous one, so the items pushed between two notes are the first's.
    let mut rules: Vec<LayerRule> = Vec::new();
    let mut pending = LayerRule::new(false, crate::renderer::material::BlendMode::Alpha);
    // Sorting groups (`ecs::sorting_group`): a whole hierarchy as one drawable.
    let groups = crate::ecs::sorting_group::SortGroups::gather(world, |l, z| sort_layers.resolve(l, z));

    // Regular sprite items (skip lit-sprite material — separate pipeline).
    // The material comes from the same query rather than a random
    // `world.get` per entity.
    let mut sprite_query = world.query::<(
        &Transform,
        &SpriteComponent,
        Option<&WorldTransform>,
        Option<&MaterialComponent>,
        Option<&crate::ecs::components::Unlit>,
        Option<&crate::ecs::components::Masked>,
        Option<&crate::ecs::carve::Carvable>,
        Option<&crate::ecs::canvas_group::CanvasGroup>,
        Option<&PaintableC>,
        Option<&SpriteTiltC>,
        Option<&JellyC>,
        Option<&SpriteOutlineC>,
        Option<&SnowCoverC>,
    )>();
    for (e, (t, s, w, m, unlit, masked, carved, group, painted, tilted, jellied, outlined, snowy)) in sprite_query.iter() {
        if !in_view_layers(world, e) {
            continue;
        }
        // A canvas group's material is the group's - its whole picture is
        // drawn through it - not its own sprite's, which would otherwise be
        // outlined inside the group as well.
        let m = if group.is_some() { None } else { m };
        let mat_id = m.map(|m| m.material_id.as_str());
        rules.resize(items.len(), pending);
        pending = LayerRule::new(unlit.is_some(), sprite_blend(m)).reading_scene(mat_id);
        let (cutout, blend) = (false, pending.base);
        let z = sort_layers.resolve(s.sort_layer.as_deref(), s.z_order);
        // Drawn already, beneath the tilemaps (`render_tilemaps_wgpu`).
        if is_backdrop(z, mat_id, unlit.is_some(), ceiling) {
            continue;
        }
        let transform = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        if let Some(v) = view
            && !v.sees_sprite(transform.position, transform.scale, s, default_size)
        {
            continue;
        }
        // An empty texture id means "tint the white pixel"; resolved here
        // rather than stored, so nothing is allocated per sprite.
        // A cutout draws through the default pipeline: its shape, not its
        // material's colour.
        // A cutout keeps its material: a graph draws the cutout itself, black
        // at its own alpha (`WgpuSpriteBatch::set_cutout`); a built-in one is
        // the default pipeline, drawn black by the vertex colour.
        // A carvable sprite draws its carved copy (`ecs::carve`), a
        // paintable one its painted copy (`ecs::paint`).
        let tex = carved
            .and_then(|c| c.state.texture.as_deref())
            .or_else(|| painted_texture(painted))
            .unwrap_or_else(|| crate::asset::resolve_texture_id(&s.texture_id));
        // A tilted sprite (`ecs::sprite_tilt`) is one quad in perspective,
        // with its back where it faces away.
        // A jelly (`ecs::jelly`) is its quad cut into a grid, each point
        // moved by its wobble.
        #[cfg(not(feature = "jelly"))]
        let _ = jellied;
        #[cfg(feature = "jelly")]
        if let Some(jl) = jellied
            && tilted.is_none()
        {
            let uv = s.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
            let (positions, uvq, colors, indices) = crate::ecs::jelly::mesh(jl, s, &transform, s.drawn_size(default_size), uv);
            items.push(InterleavedItem::Skinned {
                g: groups.of(world, e),
                z,
                tex,
                mat: mat_id,
                params: boxed_params(material_param_bits(m)),
                tris: SkinnedTriangles { positions, uvq, colors, indices },
                blend,
                cutout,
                order: s.y_sort_key(transform.position.y),
                mask: masks.index(masked),
            });
            continue;
        }
        #[cfg(not(feature = "sprite_tilt"))]
        let _ = tilted;
        #[cfg(feature = "sprite_tilt")]
        if let Some(tilt) = tilted {
            let uv = s.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
            let q = crate::ecs::sprite_tilt::quad(s, tilt, &transform, s.drawn_size(default_size), uv);
            let tex = if q.back && !tilt.back_texture.is_empty() {
                crate::asset::resolve_texture_id(&tilt.back_texture)
            } else {
                tex
            };
            items.push(InterleavedItem::Skinned {
                g: groups.of(world, e),
                z,
                tex,
                mat: mat_id,
                params: boxed_params(material_param_bits(m)),
                tris: SkinnedTriangles { positions: q.positions, uvq: q.uvq, colors: q.colors, indices: q.indices },
                blend,
                cutout,
                order: 0,
                mask: masks.index(masked),
            });
            continue;
        }
        // An outline (`ecs::sprite_outline`): the quad grown by the ring's
        // reach, just behind the sprite (one place earlier in its z), drawn
        // by the default shader's outline mode - unlit, as a glow is.
        #[cfg(not(feature = "sprite_outline"))]
        let _ = outlined;
        #[cfg(feature = "sprite_outline")]
        if let Some(o) = outlined
            && o.enabled
            && o.reach() > 0.0
            && s.fill.is_none()
        {
            let uv = s.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
            let size = s.drawn_size(default_size);
            let q = crate::ecs::sprite_outline::quad(s, &transform, size, uv, o.reach());
            let alpha = o.alpha_at(crate::profiling::now_ms() / 1000.0);
            rules.resize(items.len(), pending);
            pending = LayerRule::new(true, crate::renderer::material::BlendMode::Alpha);
            items.push(InterleavedItem::Skinned {
                g: groups.of(world, e),
                z,
                tex,
                mat: None,
                params: boxed_params(crate::ecs::sprite_outline::params(o, size, transform.scale, uv, alpha)),
                tris: SkinnedTriangles {
                    positions: q.iter().map(|c| c.0).collect(),
                    uvq: q.iter().map(|c| c.1).collect(),
                    colors: vec![[1.0; 4]; 4],
                    indices: vec![0, 1, 2, 0, 2, 3],
                },
                blend: crate::renderer::material::BlendMode::Alpha,
                cutout: false,
                mask: masks.index(masked),
                order: s.y_sort_key(transform.position.y).saturating_sub(1),
            });
            rules.resize(items.len(), pending);
            pending = LayerRule::new(unlit.is_some(), sprite_blend(m)).reading_scene(mat_id);
        }
        // Snow lying on it (`ecs::snow`): its picture over the sprite's quad,
        // one place later in its z, lit as the sprite is.
        #[cfg(not(feature = "snow"))]
        let _ = snowy;
        #[cfg(feature = "snow")]
        if let Some(snow) = snowy
            && crate::ecs::snow::shows(snow)
            && s.fill.is_none()
        {
            let uv = s.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
            let size = s.drawn_size(default_size);
            let q = crate::ecs::sprite_quad::grown(s, &transform, size, uv, 0.0);
            rules.resize(items.len(), pending);
            pending = LayerRule::new(unlit.is_some(), crate::renderer::material::BlendMode::Alpha);
            items.push(InterleavedItem::Skinned {
                g: groups.of(world, e),
                z,
                tex: snow.picture.as_str(),
                mat: None,
                params: None,
                tris: SkinnedTriangles {
                    positions: q.iter().map(|c| c.0).collect(),
                    uvq: q.iter().map(|c| c.1).collect(),
                    colors: vec![[1.0; 4]; 4],
                    indices: vec![0, 1, 2, 0, 2, 3],
                },
                blend: crate::renderer::material::BlendMode::Alpha,
                cutout: false,
                mask: masks.index(masked),
                order: s.y_sort_key(transform.position.y).saturating_add(1),
            });
            rules.resize(items.len(), pending);
            pending = LayerRule::new(unlit.is_some(), sprite_blend(m)).reading_scene(mat_id);
        }
        items.push(InterleavedItem::Sprite {
            g: groups.of(world, e),
            z,
            tex,
            mat: mat_id,
            params: boxed_params(material_param_bits(m)),
            blend,
            cutout,
            mask: masks.index(masked),
            transform,
            sprite: s,
        });
    }

    // Silhouettes (`ecs::silhouette`): a thing an occluder hides, drawn again
    // over the occluder - outlined, unlit, clipped to the occluder's pixels.
    #[cfg(feature = "silhouette")]
    let silhouettes = crate::ecs::silhouette::hidden_by(world, default_size);
    #[cfg(feature = "silhouette")]
    let mut silhouette_query = world.query::<(
        &Transform,
        &SpriteComponent,
        Option<&WorldTransform>,
        &crate::ecs::silhouette::Silhouette,
    )>();
    #[cfg(feature = "silhouette")]
    let silhouette_view = silhouette_query.view();
    #[cfg(feature = "silhouette")]
    for (hidden, wall) in silhouettes {
        if !in_view_layers(world, hidden) {
            continue;
        }
        let Some((t, s, w, style)) = silhouette_view.get(hidden) else { continue };
        let Ok(ws) = world.get::<&SpriteComponent>(wall) else { continue };
        let wall_at = world
            .get::<&WorldTransform>(wall)
            .map(|w| (*w).clone())
            .or_else(|_| world.get::<&Transform>(wall).map(|t| WorldTransform::from(&*t)))
            .unwrap_or_default();
        let occluder = world.get::<&crate::ecs::silhouette::SilhouetteOccluder>(wall).map(|o| (*o).clone()).unwrap_or_default();
        let (shape, at) = crate::ecs::silhouette::mask_of(
            &ws,
            &wall_at,
            &occluder,
            default_size,
            crate::asset::resolve_texture_id(&ws.texture_id),
        );
        let z = sort_layers.resolve(ws.sort_layer.as_deref(), ws.z_order).saturating_add(1);
        drop(ws);
        rules.resize(items.len(), pending);
        pending = LayerRule::new(true, crate::renderer::material::BlendMode::Alpha);
        items.push(InterleavedItem::Sprite {
            // Sorted as itself, not in its owner's group: it belongs over
            // the wall, whatever the thief is part of.
            g: crate::ecs::sorting_group::SortGroup { id: 0, z, y: 0 },
            z,
            tex: crate::asset::resolve_texture_id(&s.texture_id),
            mat: None,
            params: boxed_params(crate::ecs::silhouette::params(style)),
            blend: crate::renderer::material::BlendMode::Alpha,
            cutout: false,
            mask: masks.index_shape(&shape, &at),
            transform: w.cloned().unwrap_or_else(|| WorldTransform::from(t)),
            sprite: s,
        });
    }

    // Meshes, under the sprite's keys. Never a backdrop: the tilemap pass
    // draws sprites only, so a mesh sorted under the ceiling would vanish.
    let mut mesh_query = world.query::<(
        &Transform,
        &Mesh2D,
        Option<&WorldTransform>,
        Option<&MaterialComponent>,
        Option<&crate::ecs::components::Unlit>,
        Option<&crate::ecs::components::Masked>,
    )>();
    for (e, (t, mesh, w, m, unlit, masked)) in mesh_query.iter() {
        if !in_view_layers(world, e) {
            continue;
        }
        let mat_id = m.map(|m| m.material_id.as_str());
        if (mat_id.is_none() && mesh.color[3] <= 0.0) || mesh.vertices.len() < 3 {
            continue;
        }
        rules.resize(items.len(), pending);
        pending = LayerRule::new(unlit.is_some(), sprite_blend(m)).reading_scene(mat_id);
        let (cutout, blend) = (false, pending.base);
        let transform = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        if !view_sees_mesh(view.as_ref(), &transform, mesh) {
            continue;
        }
        items.push(InterleavedItem::Mesh {
            g: groups.of(world, e),
            z: sort_layers.resolve(mesh.sort_layer.as_deref(), mesh.z_order),
            tex: crate::asset::resolve_texture_id(&mesh.texture_id),
            mat: mat_id,
            params: boxed_params(material_param_bits(m)),
            blend,
            cutout,
            mask: masks.index(masked),
            transform,
            mesh,
        });
    }

    // Vector paths, lit like the sprites they now sort among - or unlit,
    // through the layers like everything else.
    let mut path_query = world.query::<(
        &Transform,
        &VectorPath,
        Option<&WorldTransform>,
        Option<&crate::ecs::components::Unlit>,
        Option<&crate::ecs::components::Masked>,
    )>();
    for (e, (t, vp, w, unlit, masked)) in path_query.iter() {
        if !in_view_layers(world, e) {
            continue;
        }
        let alpha = crate::renderer::material::BlendMode::Alpha;
        rules.resize(items.len(), pending);
        pending = LayerRule::new(unlit.is_some(), alpha);
        let (cutout, blend) = (false, pending.base);
        let parts = crate::renderer::vector_path::tessellate(vp);
        if parts.is_empty() {
            continue;
        }
        let transform = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        if !view_sees_path(view.as_ref(), &transform, &parts) {
            continue;
        }
        items.push(InterleavedItem::Path {
            g: groups.of(world, e),
            z: sort_layers.resolve(vp.sort_layer.as_deref(), vp.z_order),
            transform,
            parts,
            blend,
            cutout,
            mask: masks.index(masked),
        });
    }

    // Skinned meshes, lit, sorted by their z like the sprite they draw as.
    #[cfg(feature = "skinned_mesh")]
    let mut skinned_query = world.query::<(
        &SkinnedMesh,
        &crate::ecs::components::SkeletonRoot,
        Option<&crate::ecs::components::Unlit>,
    )>();
    #[cfg(feature = "skinned_mesh")]
    for (root, (mesh, _, unlit)) in skinned_query.iter() {
        if !in_view_layers(world, root) {
            continue;
        }
        let alpha = crate::renderer::material::BlendMode::Alpha;
        rules.resize(items.len(), pending);
        pending = LayerRule::new(unlit.is_some(), alpha);
        let (cutout, blend) = (false, pending.base);
        let Some(tris) = skin(world, root, mesh) else { continue };
        items.push(InterleavedItem::Skinned {
            mat: None,
            params: None,
            g: groups.of(world, root),
            z: sort_layers.resolve(None, mesh.z_order),
            tex: crate::asset::resolve_texture_id(&mesh.texture_id),
            tris,
            blend,
            cutout,
            order: 0,
            mask: 0,
        });
    }

    // Trails: a ribbon from where the entity is now back through the points
    // it left, drawn as world-space triangles like a skinned mesh.
    #[cfg(feature = "trails")]
    let mut trail_query = world.query::<(
        &Transform,
        &crate::ecs::components::Trail,
        Option<&WorldTransform>,
        Option<&crate::ecs::components::Unlit>,
    )>();
    #[cfg(feature = "trails")]
    for (e, (t, trail, w, unlit)) in trail_query.iter() {
        if !in_view_layers(world, e) {
            continue;
        }
        rules.resize(items.len(), pending);
        pending = LayerRule::new(unlit.is_some(), trail.blend_mode);
        let (cutout, blend) = (false, pending.base);
        let head = w.map_or(t.position, |w| w.position);
        let Some(r) = crate::ecs::trails::ribbon(trail, head) else { continue };
        if let Some(v) = view {
            let (centre, radius) = r.bounds();
            if !v.sees_disc(centre, radius) {
                continue;
            }
        }
        items.push(InterleavedItem::Skinned {
            mat: None,
            params: None,
            g: groups.of(world, e),
            z: sort_layers.resolve(trail.sort_layer.as_deref(), trail.z_order),
            tex: crate::asset::resolve_texture_id(&trail.texture_id),
            tris: SkinnedTriangles { positions: r.positions, uvq: r.uvq, colors: r.colors, indices: r.indices },
            blend,
            cutout,
            order: 0,
            mask: 0,
        });
    }

    // Spline shapes (`renderer::spline_shape`): the fill, then the edge over
    // it, each a list of world triangles of its texture, lit like a sprite.
    #[cfg(feature = "spline_shape")]
    let mut spline_query = world.query::<(
        &Transform,
        &crate::renderer::spline_shape::SplineShape,
        Option<&WorldTransform>,
        Option<&crate::ecs::components::Unlit>,
        Option<&MaterialComponent>,
    )>();
    #[cfg(feature = "spline_shape")]
    for (e, (t, shape, w, unlit, m)) in spline_query.iter() {
        if !in_view_layers(world, e) {
            continue;
        }
        rules.resize(items.len(), pending);
        pending = LayerRule::new(unlit.is_some(), sprite_blend(m));
        let (cutout, blend) = (false, pending.base);
        let wt = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        let mesh = shape.mesh();
        if let Some(v) = view
            && !mesh.polygon.is_empty()
        {
            let lo = mesh.polygon.iter().fold(Vec2::splat(f32::MAX), |a, p| a.min(*p));
            let hi = mesh.polygon.iter().fold(Vec2::splat(f32::MIN), |a, p| a.max(*p));
            let reach = (hi - lo).length() * 0.5 * wt.scale.abs().max_element() + shape.edge_width;
            if !v.sees_disc(wt.apply((lo + hi) * 0.5), reach) {
                continue;
            }
        }
        let z = sort_layers.resolve(shape.sort_layer.as_deref(), shape.z_order);
        let layers = [(0, &shape.fill_texture, &mesh.fill, shape.fill_color), (1, &shape.edge_texture, &mesh.edge, shape.edge_color)];
        for (order, tex, tris, color) in layers {
            // No fill when its colour has no alpha; no edge without a texture.
            if tris.is_empty() || color[3] <= 0.0 || (order == 1 && tex.is_empty()) {
                continue;
            }
            let n = tris.len();
            items.push(InterleavedItem::Skinned {
            mat: None,
            params: None,
                g: groups.of(world, e),
                z,
                tex: crate::asset::resolve_texture_id(tex),
                tris: SkinnedTriangles {
                    positions: tris.iter().map(|(p, _)| wt.apply(*p)).collect(),
                    uvq: tris.iter().map(|(_, uv)| [uv[0], uv[1], 1.0]).collect(),
                    colors: vec![color; n],
                    indices: (0..n as u32).collect(),
                },
                blend,
                cutout,
                order,
                mask: 0,
            });
        }
    }

    // Smooth terrain (`ecs::smooth_terrain`): its fill and edge strip.
    #[cfg(feature = "smooth_terrain")]
    let mut terrain_query = world.query::<(
        &Transform,
        &crate::ecs::smooth_terrain::SmoothTerrain,
        Option<&WorldTransform>,
        Option<&crate::ecs::components::Unlit>,
        Option<&MaterialComponent>,
    )>();
    #[cfg(feature = "smooth_terrain")]
    for (e, (t, terrain, w, unlit, m)) in terrain_query.iter() {
        if !in_view_layers(world, e) || terrain.rev == 0 {
            continue;
        }
        rules.resize(items.len(), pending);
        pending = LayerRule::new(unlit.is_some(), sprite_blend(m));
        let (cutout, blend) = (false, pending.base);
        let wt = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        let mesh = terrain.mesh();
        let z = sort_layers.resolve(terrain.sort_layer.as_deref(), terrain.z_order);
        let layers = [(0, &terrain.fill_texture, &mesh.fill, terrain.fill_color), (1, &terrain.edge_texture, &mesh.edge, terrain.edge_color)];
        for (order, tex, tris, color) in layers {
            if tris.is_empty() || color[3] <= 0.0 || (order == 1 && tex.is_empty()) {
                continue;
            }
            let n = tris.len();
            items.push(InterleavedItem::Skinned {
                mat: None,
                params: None,
                g: groups.of(world, e),
                z,
                tex: crate::asset::resolve_texture_id(tex),
                tris: SkinnedTriangles {
                    positions: tris.iter().map(|(p, _)| wt.apply(*p)).collect(),
                    uvq: tris.iter().map(|(_, uv)| [uv[0], uv[1], 1.0]).collect(),
                    colors: vec![color; n],
                    indices: (0..n as u32).collect(),
                },
                blend,
                cutout,
                order,
                mask: 0,
            });
        }
    }

    // Lines (`renderer::line2d`): triangles in world space, drawn as a
    // trail's are.
    #[cfg(feature = "line2d")]
    let mut line_query = world.query::<(
        &Transform,
        &crate::renderer::line2d::Line2D,
        Option<&WorldTransform>,
        Option<&crate::ecs::components::Unlit>,
    )>();
    #[cfg(feature = "line2d")]
    for (e, (t, line, w, unlit)) in line_query.iter() {
        if !in_view_layers(world, e) {
            continue;
        }
        rules.resize(items.len(), pending);
        pending = LayerRule::new(unlit.is_some(), line.blend_mode);
        let (cutout, blend) = (false, pending.base);
        let Some(mesh) = crate::renderer::line2d::line_mesh(line) else { continue };
        let wt = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        let positions: Vec<Vec2> = mesh.positions.iter().map(|p| wt.apply(*p)).collect();
        if let Some(v) = view {
            let lo = positions.iter().fold(Vec2::splat(f32::MAX), |a, p| a.min(*p));
            let hi = positions.iter().fold(Vec2::splat(f32::MIN), |a, p| a.max(*p));
            if !v.sees_disc((lo + hi) * 0.5, (hi - lo).length() * 0.5) {
                continue;
            }
        }
        items.push(InterleavedItem::Skinned {
            mat: None,
            params: None,
            g: groups.of(world, e),
            z: sort_layers.resolve(line.sort_layer.as_deref(), line.z_order),
            tex: crate::asset::resolve_texture_id(&line.texture_id),
            tris: SkinnedTriangles { positions, uvq: mesh.uvq, colors: mesh.colors, indices: mesh.indices },
            blend,
            cutout,
            order: 0,
            mask: 0,
        });
    }

    // Shadows laid on the ground (`ecs::cast_shadow`): the sprite's own
    // silhouette sheared from its foot along each light, fading to its tip.
    #[cfg(feature = "cast_shadow")]
    let shadow_lights = if world.query::<&crate::ecs::cast_shadow::CastShadow>().iter().next().is_some() {
        crate::ecs::cast_shadow::lights(world)
    } else {
        Vec::new()
    };
    #[cfg(feature = "cast_shadow")]
    let mut cast_query = world.query::<(
        &Transform,
        &SpriteComponent,
        &crate::ecs::cast_shadow::CastShadow,
        Option<&WorldTransform>,
    )>();
    #[cfg(feature = "cast_shadow")]
    for (e, (t, s, shadow, w)) in cast_query.iter() {
        if shadow_lights.is_empty() || s.color[3] <= 0.0 || !in_view_layers(world, e) {
            continue;
        }
        let wt = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        let size = s.drawn_size(default_size);
        let (ax, ay) = (s.anchor[0], s.anchor[1]);
        let foot_l = wt.apply(Vec2::new(-ax * size.x, -ay * size.y));
        let foot_r = wt.apply(Vec2::new((1.0 - ax) * size.x, -ay * size.y));
        let tall = foot_l.distance(wt.apply(Vec2::new(-ax * size.x, (1.0 - ay) * size.y)));
        let uv = s.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        for q in crate::ecs::cast_shadow::quads(shadow, &shadow_lights, foot_l, foot_r, tall) {
            if let Some(v) = view {
                let c = (q.corners[0] + q.corners[2]) * 0.5;
                if !v.sees_disc(c, (q.corners[2] - q.corners[0]).length()) {
                    continue;
                }
            }
            let a = (shadow.opacity * q.strength.min(1.0) * s.color[3]).clamp(0.0, 1.0);
            let [r, gg, b] = shadow.color;
            let (foot, tip) = ([r, gg, b, a], [r, gg, b, a * shadow.fade.clamp(0.0, 1.0)]);
            rules.resize(items.len(), pending);
            pending = LayerRule::shadow();
            items.push(InterleavedItem::Skinned {
                mat: None,
                params: None,
                g: groups.of(world, e),
                z: sort_layers.resolve(s.sort_layer.as_deref(), s.z_order + shadow.z_offset),
                tex: crate::asset::resolve_texture_id(&s.texture_id),
                tris: SkinnedTriangles {
                    // The foot takes the texture's bottom row, the tip its top.
                    positions: q.corners.to_vec(),
                    uvq: vec![[uv[0], uv[3], 1.0], [uv[2], uv[3], 1.0], [uv[2], uv[1], 1.0], [uv[0], uv[1], 1.0]],
                    colors: vec![foot, foot, tip, tip],
                    indices: vec![0, 1, 2, 0, 2, 3],
                },
                blend: crate::renderer::material::BlendMode::Alpha,
                cutout: false,
                mask: 0,
                order: 0,
            });
        }
    }

    // Mode 7 planes (`ecs::mode7`): the ground in perspective, one quad
    // with a perspective weight a corner.
    #[cfg(feature = "mode7")]
    let mut mode7_query = world.query::<(
        &Transform,
        &crate::ecs::mode7::Mode7,
        Option<&WorldTransform>,
        Option<&crate::ecs::components::Unlit>,
    )>();
    #[cfg(feature = "mode7")]
    for (e, (t, m, w, unlit)) in mode7_query.iter() {
        if m.texture_id.is_empty() || !in_view_layers(world, e) {
            continue;
        }
        let centre = w.map_or(t.position, |w| w.position);
        let (positions, uvq, colors, indices) = crate::ecs::mode7::quad(m, centre);
        if positions.is_empty() {
            continue;
        }
        rules.resize(items.len(), pending);
        pending = LayerRule::new(unlit.is_some(), crate::renderer::material::BlendMode::Alpha);
        items.push(InterleavedItem::Skinned {
            mat: None,
            params: None,
            g: groups.of(world, e),
            z: sort_layers.resolve(None, m.z_order),
            tex: crate::asset::resolve_texture_id(&m.texture_id),
            tris: SkinnedTriangles { positions, uvq, colors, indices },
            blend: crate::renderer::material::BlendMode::Alpha,
            cutout: false,
            mask: 0,
            order: 0,
        });
    }

    // Afterimages (`ecs::afterimage`): the poses a sprite left behind, a
    // run of quads a texture, just under the sprite in its own z.
    #[cfg(feature = "afterimage")]
    let mut echo_query = world.query::<(
        &Transform,
        &SpriteComponent,
        &crate::ecs::afterimage::Afterimage,
        Option<&WorldTransform>,
        Option<&crate::ecs::components::Unlit>,
    )>();
    #[cfg(feature = "afterimage")]
    for (e, (t, s, a, w, unlit)) in echo_query.iter() {
        if a.echoes.is_empty() || !in_view_layers(world, e) {
            continue;
        }
        let y = w.map_or(t.position.y, |w| w.position.y);
        let z = sort_layers.resolve(s.sort_layer.as_deref(), s.z_order);
        let params = if a.flat { boxed_params(crate::ecs::afterimage::flat_params(a)) } else { None };
        for run in crate::ecs::afterimage::runs(a) {
            rules.resize(items.len(), pending);
            pending = LayerRule::new(unlit.is_some(), a.blend_mode);
            items.push(InterleavedItem::Skinned {
                mat: None,
                params: params.clone(),
                g: groups.of(world, e),
                z,
                tex: crate::asset::resolve_texture_id(run.texture.as_str()),
                tris: SkinnedTriangles { positions: run.positions, uvq: run.uvq, colors: run.colors, indices: run.indices },
                blend: a.blend_mode,
                cutout: false,
                mask: 0,
                order: s.y_sort_key(y).saturating_sub(1),
            });
        }
    }

    // Shadows thrown on a 3D stage's floor (`ecs::stage`): the sprite's
    // own frame as a flat dark silhouette, in the floor band.
    #[cfg(feature = "stage")]
    let mut stage_shadow_query = world.query::<(&SpriteComponent, &crate::ecs::stage::StageBillboard)>();
    #[cfg(feature = "stage")]
    let shadow_params = boxed_params(crate::ecs::sprite_quad::silhouette_params([0.0, 0.0, 0.0, 1.0], 0.0, 1.0));
    #[cfg(feature = "stage")]
    for (e, (s, b)) in stage_shadow_query.iter() {
        if b.shadow_quads.is_empty() || s.color[3] <= 0.0 || !in_view_layers(world, e) {
            continue;
        }
        let [u0, v0, u1, v1] = s.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let mut tris = SkinnedTriangles::default();
        for quad in &b.shadow_quads {
            let base = tris.positions.len() as u32;
            for i in 0..4 {
                let [uq, vq, q] = quad.uvq[i];
                tris.positions.push(quad.corners[i]);
                // The frame's 0..1 into the sprite's own rect, still times q.
                tris.uvq.push([u0 * q + (u1 - u0) * uq, v0 * q + (v1 - v0) * vq, q]);
                tris.colors.push([0.0, 0.0, 0.0, quad.alpha[i] * s.color[3]]);
            }
            tris.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        rules.resize(items.len(), pending);
        pending = LayerRule::new(false, crate::renderer::material::BlendMode::Alpha);
        items.push(InterleavedItem::Skinned {
            mat: None,
            params: shadow_params.clone(),
            g: groups.of(world, e),
            z: b.shadow_quads[0].z,
            tex: crate::asset::resolve_texture_id(&s.texture_id),
            tris,
            blend: crate::renderer::material::BlendMode::Alpha,
            cutout: false,
            mask: 0,
            order: 0,
        });
    }

    // Solids' shadows on a 3D stage's floor (`ecs::stage::StageCaster`):
    // one flat dark shape a light, under the figures' own shadows.
    #[cfg(feature = "stage")]
    let mut caster_query = world.query::<&crate::ecs::stage::StageCaster>();
    #[cfg(feature = "stage")]
    for (e, c) in caster_query.iter() {
        if c.shadow.indices.is_empty() || !in_view_layers(world, e) {
            continue;
        }
        rules.resize(items.len(), pending);
        pending = LayerRule::new(false, crate::renderer::material::BlendMode::Alpha);
        let s = &c.shadow;
        items.push(InterleavedItem::Skinned {
            mat: None,
            params: None,
            g: groups.of(world, e),
            z: s.z,
            tex: crate::asset::WHITE_TEXTURE_ID,
            tris: SkinnedTriangles {
                positions: s.positions.clone(),
                uvq: vec![[0.5, 0.5, 1.0]; s.positions.len()],
                colors: s.colors.clone(),
                indices: s.indices.clone(),
            },
            blend: crate::renderer::material::BlendMode::Alpha,
            cutout: false,
            mask: 0,
            order: 0,
        });
    }

    // Sprite stacks (`ecs::sprite_stack`): a model's slices raised up the
    // screen, sorted by where the foot is on the screen.
    #[cfg(feature = "sprite_stack")]
    let stack_up = crate::ecs::sprite_stack::screen_up(view.map(|v| v.corners));
    #[cfg(feature = "sprite_stack")]
    let mut stack_query = world.query::<(
        &Transform,
        &crate::ecs::sprite_stack::SpriteStack,
        Option<&WorldTransform>,
        Option<&crate::ecs::components::Unlit>,
    )>();
    #[cfg(feature = "sprite_stack")]
    for (e, (t, stack, w, unlit)) in stack_query.iter() {
        if stack.color[3] <= 0.0 || stack.texture_id.is_empty() || !in_view_layers(world, e) {
            continue;
        }
        let wt = w.cloned().unwrap_or_else(|| WorldTransform::from(t));
        if let Some(v) = view
            && !v.sees_disc(wt.position, crate::ecs::sprite_stack::reach(stack, wt.scale))
        {
            continue;
        }
        rules.resize(items.len(), pending);
        pending = LayerRule::new(unlit.is_some(), crate::renderer::material::BlendMode::Alpha);
        let (positions, uvq, colors, indices) = crate::ecs::sprite_stack::quads(stack, &wt, stack_up);
        items.push(InterleavedItem::Skinned {
            mat: None,
            params: None,
            g: groups.of(world, e),
            z: sort_layers.resolve(None, stack.z_order),
            tex: crate::asset::resolve_texture_id(&stack.texture_id),
            tris: SkinnedTriangles { positions, uvq, colors, indices },
            blend: crate::renderer::material::BlendMode::Alpha,
            cutout: false,
            mask: 0,
            order: if stack.y_sort { crate::ecs::sprite_stack::screen_order(wt.position, stack_up) } else { 0 },
        });
    }

    // Liquids (`ecs::fluid`): a quad a particle, in world space, one item a
    // fluid - a `canvas_group` round it with a `threshold` makes one surface.
    #[cfg(feature = "fluid")]
    let mut fluid_query = world.query::<(&crate::ecs::fluid::Fluid, Option<&crate::ecs::components::Unlit>)>();
    #[cfg(feature = "fluid")]
    for (e, (fluid, unlit)) in fluid_query.iter() {
        if fluid.particles.is_empty() || !in_view_layers(world, e) {
            continue;
        }
        rules.resize(items.len(), pending);
        pending = LayerRule::new(unlit.is_some(), fluid.blend_mode);
        let (cutout, blend) = (false, pending.base);
        let (positions, uvq, colors, indices) = crate::ecs::fluid::quads(fluid);
        items.push(InterleavedItem::Skinned {
            mat: None,
            params: None,
            g: groups.of(world, e),
            z: sort_layers.resolve(fluid.sort_layer.as_deref(), fluid.z_order),
            tex: crate::asset::resolve_texture_id(&fluid.texture_id),
            tris: SkinnedTriangles { positions, uvq, colors, indices },
            blend,
            cutout,
            order: 0,
            mask: 0,
        });
    }

    // Falling sand's glow: unlit and additive over the simulation's sprite,
    // so bloom picks up its fire and lava.
    #[cfg(feature = "pixel_sim")]
    for (e, corners, k, z, tex) in &pixel_glows {
        if !in_view_layers(world, *e) {
            continue;
        }
        rules.resize(items.len(), pending);
        let blend = crate::renderer::material::BlendMode::Additive;
        pending = LayerRule::new(true, blend);
        items.push(InterleavedItem::Skinned {
            mat: None,
            params: None,
            g: groups.of(world, *e),
            z: *z,
            tex: tex.as_str(),
            tris: SkinnedTriangles {
                positions: corners.to_vec(),
                uvq: vec![[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]],
                colors: vec![[*k, *k, *k, 1.0]; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
            },
            blend,
            cutout: false,
            mask: 0,
            order: 1,
        });
    }

    // Water (`ecs::water`): the body from its moving surface down, and its
    // foam, one item a water.
    #[cfg(feature = "water")]
    let mut water_query = world.query::<(
        &crate::ecs::water::WaterSurface,
        &Transform,
        Option<&WorldTransform>,
        Option<&crate::ecs::components::Unlit>,
    )>();
    #[cfg(feature = "water")]
    for (e, (water, t, w, unlit)) in water_query.iter() {
        if !in_view_layers(world, e) {
            continue;
        }
        let at = w.map_or(t.position, |w| w.position);
        rules.resize(items.len(), pending);
        pending = LayerRule::new(unlit.is_some(), water.blend_mode);
        let (cutout, blend) = (false, pending.base);
        let (positions, uvq, colors, indices) = crate::ecs::water::mesh(water, at);
        items.push(InterleavedItem::Skinned {
            mat: None,
            params: None,
            g: groups.of(world, e),
            z: sort_layers.resolve(water.sort_layer.as_deref(), water.z_order),
            tex: crate::asset::WHITE_TEXTURE_ID,
            tris: SkinnedTriangles { positions, uvq, colors, indices },
            blend,
            cutout,
            order: 0,
            mask: 0,
        });
    }

    // Water running along a curve (`ecs::flow`): a ribbon cut where its
    // texture repeats, the cuts running downstream.
    #[cfg(feature = "flow")]
    let mut flow_query = world.query::<(&crate::ecs::flow::FlowPath, Option<&crate::ecs::components::Unlit>)>();
    #[cfg(feature = "flow")]
    for (e, (f, unlit)) in flow_query.iter() {
        if !in_view_layers(world, e) {
            continue;
        }
        let Some(path) = crate::ecs::path_follow::measured_path(world, &f.path) else { continue };
        let (positions, uvq, colors, indices) = crate::ecs::flow::mesh(f, &path);
        if indices.is_empty() {
            continue;
        }
        rules.resize(items.len(), pending);
        pending = LayerRule::new(unlit.is_some(), crate::renderer::material::BlendMode::Alpha);
        let (cutout, blend) = (false, pending.base);
        let tex = if f.texture.is_empty() { crate::asset::WHITE_TEXTURE_ID } else { crate::asset::resolve_texture_id(&f.texture) };
        items.push(InterleavedItem::Skinned {
            mat: None,
            params: None,
            g: groups.of(world, e),
            z: f.z_order,
            tex,
            tris: SkinnedTriangles { positions, uvq, colors, indices },
            blend,
            cutout,
            order: 0,
            mask: 0,
        });
    }

    // Lightning (`ecs::lightning`): a strike's glow and core, additive and
    // unlit, one item a bolt.
    #[cfg(feature = "lightning")]
    let mut bolt_query = world.query::<&crate::ecs::lightning::Lightning>();
    #[cfg(feature = "lightning")]
    for (e, l) in bolt_query.iter() {
        if l.strike.is_none() || !in_view_layers(world, e) {
            continue;
        }
        let (positions, uvq, colors, indices) = crate::ecs::lightning::mesh(l);
        if indices.is_empty() {
            continue;
        }
        let additive = crate::renderer::material::BlendMode::Additive;
        rules.resize(items.len(), pending);
        pending = LayerRule::new(true, additive);
        items.push(InterleavedItem::Skinned {
            mat: None,
            params: None,
            g: groups.of(world, e),
            z: l.z_order,
            tex: crate::asset::WHITE_TEXTURE_ID,
            tris: SkinnedTriangles { positions, uvq, colors, indices },
            blend: additive,
            cutout: false,
            mask: 0,
            order: 0,
        });
    }

    // Canvas groups' pictures (`ecs::canvas_group`): each group's members,
    // drawn this frame into its texture by its own pass, as one quad over
    // the camera's view at the group's z - premultiplied, since the picture
    // was drawn onto transparent, and unlit, since its members were lit.
    if let Some(v) = view
        && v.pass == crate::renderer::camera::CameraPass::Main
    {
        for (g, group) in world.query::<&crate::ecs::canvas_group::CanvasGroup>().iter() {
            if group.opacity <= 0.0 || crate::ecs::canvas_group::member_of(g) != crate::ecs::canvas_group::group_key(g) {
                continue;
            }
            let own = canvas_mats
                .iter()
                .find(|m| m.0 == g)
                .map_or(crate::renderer::material::BlendMode::PremultipliedAlpha, |m| m.3);
            let a = group.opacity.clamp(0.0, 1.0) * group.tint[3];
            let c = [group.tint[0] * a, group.tint[1] * a, group.tint[2] * a, a];
            let [tl, tr, br, bl] = v.corners;
            canvas_targets.push((g, sort_layers.resolve(group.sort_layer.as_deref(), group.z_order), false, own, c, [tl, tr, br, bl]));
        }
    }

    for (g, z, cutout, blend, c, [tl, tr, br, bl]) in canvas_targets.drain(..) {
        let id = crate::ecs::canvas_group::target_id(g);
        let Some(tex) = canvas_ids.iter().find(|t| **t == id) else { continue };
        // Its picture was lit already: unlit, in the group's own blend.
        rules.resize(items.len(), pending);
        pending = LayerRule::new(true, blend);
        let own = canvas_mats.iter().find(|m| m.0 == g);
        let mut bits = own.map_or([0; 16], |m| m.2);
        if own.is_none()
            && let Some(&(_, edge, soft)) = canvas_thresholds.iter().find(|t| t.0 == g)
        {
            bits[4] = edge.to_bits();
            bits[5] = soft.to_bits();
        }
        items.push(InterleavedItem::Skinned {
            mat: own.map(|m| m.1.as_str()),
            params: boxed_params(bits),
            g: crate::ecs::sorting_group::SortGroup::NONE,
            z,
            tex: tex.as_str(),
            tris: SkinnedTriangles {
                positions: vec![tl, tr, br, bl],
                uvq: vec![[0.0, 0.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 1.0], [0.0, 1.0, 1.0]],
                colors: vec![c; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
            },
            blend,
            cutout,
            order: 0,
            mask: 0,
        });
    }

    // Mirrors' reflections (`ecs::mirror`): each mirror's view of what is in
    // front of it, drawn over as deep behind it, flipped across the line. In
    // views too, so a mirror shows the mirrors it faces; not in a canvas
    // group's pass, which draws only the group.
    #[cfg(any(feature = "mirror", feature = "portal"))]
    let pass = view.map(|v| v.pass);
    #[cfg(any(feature = "mirror", feature = "portal"))]
    let group_pass = matches!(pass, Some(crate::renderer::camera::CameraPass::CanvasGroup(_)));
    #[cfg(feature = "mirror")]
    for (m, tex) in mirror_ids.iter().filter(|_| !group_pass) {
        if pass == Some(crate::renderer::camera::CameraPass::Mirror(crate::ecs::mirror::mirror_key(*m)))
            || !in_view_layers(world, *m)
        {
            continue;
        }
        let Some((mirror, place)) = crate::ecs::mirror::place_of(world, *m) else { continue };
        let a = mirror.tint[3].clamp(0.0, 1.0);
        let c = [mirror.tint[0] * a, mirror.tint[1] * a, mirror.tint[2] * a, a];
        let (positions, uv) = place.reflection();
        // Its picture was lit already: unlit, and premultiplied, since it
        // was drawn onto transparent.
        rules.resize(items.len(), pending);
        let blend = crate::renderer::material::BlendMode::PremultipliedAlpha;
        pending = LayerRule::new(true, blend);
        items.push(InterleavedItem::Skinned {
            mat: None,
            params: None,
            g: crate::ecs::sorting_group::SortGroup::NONE,
            z: sort_layers.resolve(mirror.sort_layer.as_deref(), mirror.z_order),
            tex: tex.as_str(),
            tris: SkinnedTriangles {
                positions: positions.to_vec(),
                uvq: uv.to_vec(),
                colors: vec![c; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
            },
            blend,
            cutout: false,
            mask: 0,
            order: 0,
        });
    }

    // Portals' windows (`ecs::portal`): each portal's view out of its
    // partner, drawn as deep behind its line, turned half round - the same
    // rules as a mirror's reflection.
    #[cfg(feature = "portal")]
    for (p, tex) in portal_ids.iter().filter(|_| !group_pass) {
        if pass == Some(crate::renderer::camera::CameraPass::Mirror(crate::ecs::portal::portal_key(*p)))
            || !in_view_layers(world, *p)
        {
            continue;
        }
        let Some((portal, place)) = crate::ecs::portal::place_of(world, *p) else { continue };
        let a = portal.tint[3].clamp(0.0, 1.0);
        let c = [portal.tint[0] * a, portal.tint[1] * a, portal.tint[2] * a, a];
        let (positions, uv) = crate::ecs::portal::window(&place);
        rules.resize(items.len(), pending);
        let blend = crate::renderer::material::BlendMode::PremultipliedAlpha;
        pending = LayerRule::new(true, blend);
        items.push(InterleavedItem::Skinned {
            mat: None,
            params: None,
            g: crate::ecs::sorting_group::SortGroup::NONE,
            z: sort_layers.resolve(portal.sort_layer.as_deref(), portal.z_order),
            tex: tex.as_str(),
            tris: SkinnedTriangles {
                positions: positions.to_vec(),
                uvq: uv.to_vec(),
                colors: vec![c; 4],
                indices: vec![0, 1, 2, 0, 2, 3],
            },
            blend,
            cutout: false,
            mask: 0,
            order: 0,
        });
    }

    // World text, placed by the world transform like sprites. Empty strings
    // are dropped here so they do not split a run. Not culled: the layout is
    // what knows a block's extent, and text is few.
    let mut text_query = world.query::<(
        &Transform,
        &TextComponent,
        Option<&WorldTransform>,
        Option<&crate::ecs::components::Unlit>,
        Option<&crate::ecs::components::Masked>,
    )>();
    for (e, (t, tc, w, unlit, masked)) in text_query.iter() {
        if !in_view_layers(world, e) {
            continue;
        }
        if tc.text.is_empty() {
            continue;
        }
        let alpha = crate::renderer::material::BlendMode::Alpha;
        rules.resize(items.len(), pending);
        pending = LayerRule::new(unlit.is_some(), alpha);
        let (cutout, blend) = (false, pending.base);
        let (font, tex) = fonts.resolve(&tc.font_id);
        let (position, scale, rotation) =
            w.map_or((t.position, t.scale.x, t.rotation), |w| (w.position, w.scale.x, w.rotation));
        // Text along a path (`ecs::text_path`): every glyph bent onto the
        // curve and turned to it, drawn as one mesh of the font's atlas.
        #[cfg(feature = "text_path")]
        if let Ok(tp) = world.get::<&crate::ecs::text_path::TextPath>(e) {
            if let Some(path) = crate::ecs::text_path::source(world, &tp.path) {
                let s = tc.font_scale * scale;
                let quads =
                    crate::renderer::text_layout::glyph_quads(font, &tc.text, Vec2::ZERO, s, 0.0, None, tc.align);
                let bent = crate::ecs::text_path::bend(
                    &quads,
                    font.line_height * s,
                    &path,
                    tp.offset,
                    tp.side,
                    tp.spacing,
                    tp.repeat,
                );
                let (positions, uvq, colors, indices) = crate::ecs::text_path::glyph_mesh(&bent, tc.color);
                if !positions.is_empty() {
                    items.push(InterleavedItem::Skinned {
                        mat: None,
                        params: None,
                        g: groups.of(world, e),
                        z: sort_layers.resolve(tc.sort_layer.as_deref(), tc.z_order),
                        tex: crate::asset::resolve_texture_id(tex),
                        tris: SkinnedTriangles { positions, uvq, colors, indices },
                        blend,
                        cutout,
                        order: 0,
                        mask: 0,
                    });
                }
            }
            continue;
        }
        // Culled by a bound the layout cannot exceed: every glyph advance as
        // one line (or the wrap width, if wider) plus one line height per
        // line, whichever way the block hangs from its anchor.
        if let Some(v) = view {
            let s = tc.font_scale * scale;
            let width = crate::renderer::text_layout::measure_line(font, &tc.text, s).max(tc.wrap_width);
            let lines = tc.text.lines().count().max(1) as f32;
            let reach = width + lines * font.line_height * s;
            if !v.sees_disc(position, reach) {
                continue;
            }
        }
        let z = sort_layers.resolve(tc.sort_layer.as_deref(), tc.z_order);
        let mask = masks.index(masked);
        let (rich, visible) = if tc.rich {
            let rt = crate::renderer::rich_text::parse(&tc.text, crate::renderer::rich_text::icon_spaces(font));
            let visible = crate::renderer::rich_text::visible_chars(&rt, tc.reveal_speed, tc.reveal.map(|r| r.elapsed));
            // Each icon shown so far: a square a line tall over its spaces.
            if !rt.icons.is_empty() {
                let s = tc.font_scale * scale;
                let wrap = (tc.wrap_width > 0.0).then_some(tc.wrap_width);
                let places =
                    crate::renderer::text_layout::glyph_places(font, &rt.plain, position, s, rotation, wrap, tc.align);
                let side = font.line_height * s * 0.9;
                for icon in rt.icons.iter().filter(|i| i.at < visible.0) {
                    let cells: Vec<Vec2> = places
                        .iter()
                        .filter(|p| p.index >= icon.at && p.index < icon.at + icon.len)
                        .map(|p| p.cell_center)
                        .collect();
                    let (Some(first), Some(last)) = (cells.first(), cells.last()) else { continue };
                    items.push(InterleavedItem::Icon {
                        g: groups.of(world, e),
                        z,
                        tex: &tc.text[icon.id.clone()],
                        center: (*first + *last) * 0.5,
                        size: Vec2::splat(side),
                        rotation,
                        color: Vec4::new(1.0, 1.0, 1.0, tc.color[3]),
                        blend,
                        cutout,
                        mask,
                    });
                }
            }
            (Some(std::rc::Rc::new(rt)), visible)
        } else if crate::renderer::rich_text::reveal_running(false, tc.reveal_speed, tc.reveal.map(|r| r.elapsed)) {
            let rt = crate::renderer::rich_text::literal(&tc.text);
            let visible = crate::renderer::rich_text::visible_chars(&rt, tc.reveal_speed, tc.reveal.map(|r| r.elapsed));
            (Some(std::rc::Rc::new(rt)), visible)
        } else {
            (None, (usize::MAX, 0.0))
        };
        // The shadow and the outline first, each the text again in one
        // colour (`TextShadow`, `TextOutline`).
        let s = tc.font_scale * scale;
        let field = font.sdf;
        let mut layer = |layer: u8, solid: Option<Vec4>, nudge: Vec2, style: TextStyle| {
            items.push(InterleavedItem::Text {
                g: groups.of(world, e),
                z,
                tex,
                font,
                text: tc,
                position,
                scale,
                rotation,
                blend,
                cutout,
                mask,
                rich: rich.clone(),
                visible,
                layer,
                solid,
                nudge,
                style,
            });
        };
        if let Some(sh) = tc.shadow {
            let style = if field {
                // Grown by its softness as well, so the blur lies outside
                // the glyph rather than half under it.
                let soften = field_units(font, s, sh.softness);
                let grow = tc.outline.map_or(0.0, |o| field_units(font, s, o.width)) + soften;
                TextStyle::Field { grow: grow.min(0.45), soften }
            } else {
                tc.outline.map_or(TextStyle::Plain, |o| TextStyle::Copies { width: o.width })
            };
            layer(0, Some(faded(sh.color, tc.color[3])), Vec2::from(sh.offset), style);
        }
        if let Some(o) = tc.outline.filter(|o| o.width > 0.0) {
            let style = if field {
                TextStyle::Field { grow: field_units(font, s, o.width), soften: 0.0 }
            } else {
                TextStyle::Copies { width: o.width }
            };
            layer(1, Some(faded(o.color, tc.color[3])), Vec2::ZERO, style);
        }
        layer(2, None, Vec2::ZERO, TextStyle::Plain);
    }

    // GPU particle emitters: one item each, drawn by the batch in one call.
    let mut gpu_query = world.query::<(
        &Transform,
        &crate::particles::gpu::GpuParticles,
        Option<&WorldTransform>,
        Option<&crate::ecs::components::Unlit>,
    )>();
    for (e, (t, g, w, unlit)) in gpu_query.iter() {
        if !in_view_layers(world, e) {
            continue;
        }
        if g.count == 0 {
            continue;
        }
        rules.resize(items.len(), pending);
        pending = LayerRule::new(unlit.is_some(), g.blend_mode);
        let blend = pending.base;
        let (position, rotation, scale) = w.map_or((t.position, t.rotation, t.scale), |w| (w.position, w.rotation, w.scale));
        items.push(InterleavedItem::Gpu {
            g: groups.of(world, e),
            z: sort_layers.resolve(g.sort_layer.as_deref(), g.z_order),
            tex: crate::asset::resolve_texture_id(&g.texture_id),
            blend,
            uniform: Box::new((
                g.uniform(position, rotation, scale, [[0.0; 4]; 4]),
                g.stateful().then(|| e.to_bits().get()),
            )),
            count: g.count,
        });
    }

    // The tiles of `y_sort` tilemap layers, each a quad sorted by its cell
    // with the y-sorted sprites of the tilemap's z.
    let mut tilemap_query = world.query::<(
        &Transform,
        &Tilemap,
        Option<&crate::tilemap::autotile::Autotile>,
        Option<&crate::ecs::components::Unlit>,
    )>();
    for (e, (t, tm, autotile, unlit)) in tilemap_query.iter() {
        if !tm.layers.iter().any(|l| l.y_sort) || !in_view_layers(world, e) {
            continue;
        }
        let alpha = crate::renderer::material::BlendMode::Alpha;
        rules.resize(items.len(), pending);
        pending = LayerRule::new(unlit.is_some(), alpha);
        let (cutout, blend) = (false, pending.base);
        let z = sort_layers.resolve(tm.sort_layer.as_deref(), tm.z_order);
        let (center, half) = view.map_or((Vec2::ZERO, Vec2::splat(f32::MAX / 4.0)), |v| (v.center, v.half));
        for l in tm.layers.iter().filter(|l| l.y_sort && l.visible && !l.collision) {
            let rule = autotile.and_then(|a| a.for_layer(&l.name));
            place_layer_tiles(tm, crate::tilemap::world_origin(world, e, t), l, rule, center, half, &mut |p| {
                let ts = &tm.tilesets[p.tileset];
                let Some(uv) = ts.tile_uv(p.local) else { return };
                items.push(InterleavedItem::Tile {
                    g: groups.of(world, e),
                    z,
                    tex: crate::asset::resolve_texture_id(&ts.texture_id),
                    center: p.center,
                    size: p.size,
                    uv,
                    // `SpriteComponent::y_sort_key`'s scale, so the two interleave.
                    y_key: (-p.foot_y * 16.0).clamp(i32::MIN as f32, i32::MAX as f32) as i32,
                    blend,
                    cutout,
                });
            });
        }
    }

    // Shadow items (one per enabled ShadowCaster). Geometry math mirrors
    // `render_shadows_wgpu` exactly — keep them in lock-step.
    // Bound to a local like the sprite query above: the items borrow from it,
    // so the QueryBorrow has to outlive the loop rather than be a temporary.
    let mut shadow_query = world.query::<(
        &Transform,
        &SpriteComponent,
        &ShadowCaster,
        Option<&MaterialComponent>,
        Option<&crate::ecs::components::Unlit>,
    )>();
    for (e, (transform, sprite, shadow, m, unlit)) in shadow_query.iter() {
        if !in_view_layers(world, e) {
            continue;
        }
        // A shadow darkens the scene; it has no part in the emission layer.
        if !shadow.enabled {
            continue;
        }
        // A backdrop sprite's shadow belongs under the tilemaps with it.
        let key = sort_layers.resolve(sprite.sort_layer.as_deref(), sprite.z_order);
        if is_backdrop(key, m.map(|m| m.material_id.as_str()), unlit.is_some(), ceiling) {
            continue;
        }
        let offset = Vec2::new(
            shadow.offset[0] * shadow.height,
            shadow.offset[1] * shadow.height,
        );
        let position = transform.position + offset;
        let uv = sprite.uv_rect.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        let base = sprite.drawn_size(default_size);
        let shadow_scale = Vec2::new(shadow.scale[0], shadow.scale[1]);
        let size = base * transform.scale * shadow_scale;
        if let Some(v) = view
            && !v.sees_disc(position, size.length())
        {
            continue;
        }
        rules.resize(items.len(), pending);
        pending = LayerRule::shadow();
        items.push(InterleavedItem::Shadow {
            g: groups.of(world, e),
            z: sort_layers.resolve(sprite.sort_layer.as_deref(), sprite.z_order),
            tex: crate::asset::resolve_texture_id(&sprite.texture_id),
            pos: position,
            size,
            rot: transform.rotation,
            skew: shadow.skew,
            color: Vec4::from(shadow.color),
            uv,
            anchor: sprite.anchor,
        });
    }

    // Each item's place in the order beside its index: the pairs are sorted
    // and walked, and the items - large - never move.
    rules.resize(items.len(), pending);
    let mut w = WorldItems { items, rules, masks, default_size };
    f(&mut w)
}

/// The world's drawables, collected once (see [`render_world_items`]).
pub struct WorldItems<'a> {
    items: Vec<InterleavedItem<'a>>,
    rules: Vec<LayerRule>,
    masks: MaskTable,
    default_size: Vec2,
}

impl WorldItems<'_> {
    /// Whether anything collected draws itself in the emission layer - the
    /// one case a lit frame needs the emission pass ([`WorldLayer::Emission`]).
    /// Asked of the items, not the world: the frame used to ask whether any
    /// entity carried `Unlit`, and every unlit thing the engine draws for an
    /// entity without one - a lightning bolt, an outline, a silhouette, a
    /// mirror's picture - vanished from a lit scene that had nothing else
    /// unlit in it. The showcase's Thunderhead hid it behind its unlit rain;
    /// the Test Lab's bare scene did not.
    pub fn draws_emission(&self) -> bool {
        self.rules.iter().take(self.items.len()).any(|r| r.unlit && !r.shadow)
    }

    /// Draw one layer of the world: what [`render_sprites_with_shadows_wgpu_materialized`]
    /// did per call - the layer's rules applied to each item (left out, a
    /// cutout, its blend), sorted, and walked into flushes - over the items
    /// collected once. Returns `(sprites, shadows)` flushed.
    pub fn draw(
        &mut self,
        layer: WorldLayer,
        batch: &mut WgpuSpriteBatch,
        mut flush: impl FnMut(&mut WgpuSpriteBatch, &str, SpriteShadowFlush<'_>),
    ) -> (usize, usize) {
        let mut active: Vec<u32> = Vec::with_capacity(self.items.len());
        for (i, (item, rule)) in self.items.iter_mut().zip(&self.rules).enumerate() {
            if rule.admits(layer, item) {
                active.push(i as u32);
            }
        }
        let items = &self.items;
        let masks = &mut self.masks;
        let default_size = self.default_size;
        let mut keys = item_keys(items, &active);
        // The band is where it sorts: a group's z for a member of one.
        keys.retain(|(k, _)| in_z_window(k.outer.0));
        if keys.is_empty() {
            return (0, 0);
        }
        // Stable, as `sort_by` was: items that tie keep their collection order.
        keys.sort_by_key(|&(k, _)| k);
        // The emission buffer starts clear and the composite reads only its
        // colour, so a cutout - black - before the first thing that glows
        // draws black onto black: drop them. Exact, and in a lit scene with
        // a few unlit things it is most of the layer.
        if layer == WorldLayer::Emission {
            let first = keys.iter().position(|(_, i)| !items[*i as usize].is_cutout()).unwrap_or(keys.len());
            keys.drain(..first);
            if keys.is_empty() {
                return (0, 0);
            }
        }

        // Stream: walk the sorted list, group by bucket key, flush each group
        // through the appropriate pipeline-bound callback.
        let mut sprite_total = 0usize;
        let mut shadow_total = 0usize;
        let mut i = 0;
        while i < keys.len() {
            // Snapshot the current bucket. We compare against this for the
            // inner-loop break; clone the strings so we don't hold an immutable
            // borrow into `items` while we mutate `batch` via &mut.
            let (cur_kind, cur_tex, cur_mat) = match &items[keys[i].1 as usize] {
                InterleavedItem::Shadow { tex, .. } => (0u8, *tex, None),
                InterleavedItem::Sprite { tex, mat, .. } | InterleavedItem::Mesh { tex, mat, .. } => (1u8, *tex, *mat),
                InterleavedItem::Path { .. } => (1u8, crate::asset::WHITE_TEXTURE_ID, None),
                InterleavedItem::Skinned { tex, mat, .. } => (1u8, *tex, *mat),
                InterleavedItem::Tile { tex, .. } => (1u8, *tex, None),
                InterleavedItem::Text { tex, .. } | InterleavedItem::Icon { tex, .. } => (2u8, *tex, None),
                InterleavedItem::Gpu { tex, .. } => (3u8, *tex, None),
            };
            let first = &items[keys[i].1 as usize];
            let cur_params = first.params();
            let cur_blend = first.blend();
            let cur_cutout = first.is_cutout();
            let cur_mask = first.mask();
            let cur_batch = keys[i].0.batch();
            batch.begin();
            batch.set_params(crate::renderer::material_params::from_bits(&cur_params));
            batch.set_blend(cur_blend);
            batch.set_cutout(cur_cutout);
            masks.apply(cur_mask, batch, &mut |b, t| flush(b, t, SpriteShadowFlush::Sprite { material_id: None }));
            let mut count = 0usize;
            while i < keys.len() {
                let item = &items[keys[i].1 as usize];
                // Kind, texture, material, parameters, blend and mask, as ranks.
                if keys[i].0.batch() != cur_batch || item.is_cutout() != cur_cutout {
                    break;
                }
                let mul = item.mul();
                // An emitter is drawn alone: the batch draws it at the flush.
                if let InterleavedItem::Gpu { uniform, count: n, .. } = item {
                    if count > 0 {
                        break;
                    }
                    batch.set_gpu_particles(Some((uniform.0, *n, uniform.1)));
                    count += 1;
                    i += 1;
                    break;
                }
                match item {
                    // Handled above: drawn alone, at the flush.
                    InterleavedItem::Gpu { .. } => {}
                    InterleavedItem::Shadow { pos, size, rot, skew, color, uv, anchor, .. } => {
                        // Shadows can flood the batch on dense scenes — mirror
                        // the standalone shadow path's mid-bucket overflow flush.
                        if batch.is_full() {
                            flush(batch, cur_tex, SpriteShadowFlush::Shadow);
                            shadow_total += count;
                            count = 0;
                            batch.begin();
                        }
                        batch.draw_sprite_anchored_skewed(
                            *pos, *size, *rot, *skew, *color, *uv, *anchor,
                        );
                    }
                    InterleavedItem::Sprite { transform, sprite, .. } => {
                        // Same as the shadow arm: a run longer than the batch
                        // holds goes out in pieces. This arm had no check, so a
                        // dense bucket drew its first 8,192 sprites and dropped
                        // the rest without a word. Multi-quad `SpriteFill` modes
                        // count as one sprite for the check; a single sprite
                        // that alone overflows a batch is reported by the batch.
                        if batch.is_full() {
                            flush(
                                batch,
                                cur_tex,
                                SpriteShadowFlush::Sprite { material_id: cur_mat },
                            );
                            sprite_total += count;
                            count = 0;
                            batch.begin();
                        }
                        queue_sprite_tinted(batch, transform, sprite, default_size, mul);
                    }
                    InterleavedItem::Mesh { transform, mesh, .. } => {
                        if batch.is_full() {
                            flush(batch, cur_tex, SpriteShadowFlush::Sprite { material_id: cur_mat });
                            sprite_total += count;
                            count = 0;
                            batch.begin();
                        }
                        queue_mesh_tinted(batch, transform, mesh, mul);
                        // A mesh bigger than a whole batch goes out in pieces.
                        while batch.has_overflow() {
                            let rest = batch.take_overflow();
                            flush(batch, cur_tex, SpriteShadowFlush::Sprite { material_id: cur_mat });
                            sprite_total += count;
                            count = 0;
                            batch.begin();
                            batch.resume_overflow(rest);
                        }
                    }
                    InterleavedItem::Path { transform, parts, .. } => {
                        if batch.is_full() {
                            flush(batch, cur_tex, SpriteShadowFlush::Sprite { material_id: cur_mat });
                            sprite_total += count;
                            count = 0;
                            batch.begin();
                        }
                        queue_path_tinted(batch, transform, parts, mul);
                        // A mesh bigger than a whole batch goes out in pieces.
                        while batch.has_overflow() {
                            let rest = batch.take_overflow();
                            flush(batch, cur_tex, SpriteShadowFlush::Sprite { material_id: cur_mat });
                            sprite_total += count;
                            count = 0;
                            batch.begin();
                            batch.resume_overflow(rest);
                        }
                    }
                    InterleavedItem::Skinned { tris, .. } => {
                        if batch.is_full() {
                            flush(batch, cur_tex, SpriteShadowFlush::Sprite { material_id: cur_mat });
                            sprite_total += count;
                            count = 0;
                            batch.begin();
                        }
                        queue_skinned_tinted(batch, tris, mul);
                        // A mesh bigger than a whole batch goes out in pieces.
                        while batch.has_overflow() {
                            let rest = batch.take_overflow();
                            flush(batch, cur_tex, SpriteShadowFlush::Sprite { material_id: cur_mat });
                            sprite_total += count;
                            count = 0;
                            batch.begin();
                            batch.resume_overflow(rest);
                        }
                    }
                    InterleavedItem::Tile { center, size, uv, .. } => {
                        if batch.is_full() {
                            flush(batch, cur_tex, SpriteShadowFlush::Sprite { material_id: None });
                            sprite_total += count;
                            count = 0;
                            batch.begin();
                        }
                        batch.draw_sprite_uv(*center, *size, 0.0, mul, *uv);
                    }
                    InterleavedItem::Icon { center, size, rotation, color, .. } => {
                        if batch.is_full() {
                            flush(batch, cur_tex, SpriteShadowFlush::Sprite { material_id: None });
                            sprite_total += count;
                            count = 0;
                            batch.begin();
                        }
                        batch.draw_sprite_uv(*center, *size, *rotation, *color * mul, [0.0, 0.0, 1.0, 1.0]);
                    }
                    InterleavedItem::Text {
                        font, text, position, scale, rotation, rich, visible, solid, nudge, style, ..
                    } => {
                        if batch.is_full() {
                            flush(batch, cur_tex, SpriteShadowFlush::Sprite { material_id: None });
                            sprite_total += count;
                            count = 0;
                            batch.begin();
                        }
                        // A bitmap font's outline is the text eight times round
                        // itself; everything else is drawn once.
                        let ring: &[Vec2] = &[
                            Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0), Vec2::new(0.0, 1.0), Vec2::new(0.0, -1.0),
                            Vec2::new(0.7, 0.7), Vec2::new(-0.7, 0.7), Vec2::new(0.7, -0.7), Vec2::new(-0.7, -0.7),
                        ];
                        let (spots, width): (&[Vec2], f32) = match style {
                            TextStyle::Copies { width } => (ring, *width),
                            _ => (&[Vec2::ZERO], 0.0),
                        };
                        for spot in spots {
                            let at = *position + *nudge + *spot * width;
                            match rich {
                                Some(rich) => {
                                    queue_rich_text(batch, font, text, rich, *visible, at, *scale, *rotation, mul, *solid);
                                }
                                None => {
                                    // `wrap_width` is in world units, the space the
                                    // glyph quads are laid out in; 0 means no wrapping.
                                    let wrap = (text.wrap_width > 0.0).then_some(text.wrap_width);
                                    let color = solid.unwrap_or(Vec4::from(text.color));
                                    queue_text_glyphs_rotated(
                                        font,
                                        batch,
                                        &text.text,
                                        at,
                                        text.font_scale * *scale,
                                        *rotation,
                                        color * mul,
                                        wrap,
                                        text.align,
                                    );
                                }
                            }
                        }
                    }
                }
                count += 1;
                i += 1;
            }
            match cur_kind {
                0 => {
                    flush(batch, cur_tex, SpriteShadowFlush::Shadow);
                    shadow_total += count;
                }
                _ => {
                    flush(
                        batch,
                        cur_tex,
                        SpriteShadowFlush::Sprite { material_id: cur_mat },
                    );
                    sprite_total += count;
                }
            }
        }
        batch.set_blend(crate::renderer::material::BlendMode::Alpha);
        batch.set_cutout(false);
        batch.set_mask(crate::renderer::mask::MaskUniform::NONE);
        // The params carry a text outline's edge to the default shader now
        // (`sprite.wgsl`), so they are put back too: left set, the next UI text
        // would draw grown.
        batch.set_params([[0.0; 4]; 4]);

        (sprite_total, shadow_total)
    }
}

/// Test-only helper: collect & sort the interleaved bucket keys for the
/// current `World`. Returns `(z, kind, tex, mat)` per item in render order.
///
/// Pure data, no wgpu — lets unit tests assert ordering without booting a
/// GPU device.
#[cfg(test)]
pub(crate) fn collect_interleaved_sort_keys(
    world: &World,
    default_size: Vec2,
) -> Vec<(i32, u8, String, Option<String>)> {
    let mut items: Vec<InterleavedItem> = Vec::new();
    // Queries bound to locals: the items borrow from them (see
    // `InterleavedItem`), so they must outlive the loops.
    let mut sprite_query = world.query::<(
        &Transform,
        &SpriteComponent,
        Option<&WorldTransform>,
        Option<&MaterialComponent>,
        Option<&crate::ecs::components::Unlit>,
    )>();
    for (_, (t, s, w, m, unlit)) in sprite_query.iter() {
        let mat_id = m.map(|m| m.material_id.as_str());
        // `Unlit` sprites are drawn after the lighting composite by
        // the world pass's layers; their shadows stay here, in the world.
        if unlit.is_some() {
            continue;
        }
        items.push(InterleavedItem::Sprite {
            g: crate::ecs::sorting_group::SortGroup::NONE,
            z: s.z_order,
            tex: &s.texture_id,
            mat: mat_id,
            params: boxed_params(material_param_bits(m)),
            blend: sprite_blend(m),
            cutout: false,
            mask: 0,
            transform: w.cloned().unwrap_or_else(|| WorldTransform::from(t)),
            sprite: s,
        });
    }
    let mut mesh_query = world.query::<(&Transform, &Mesh2D, Option<&MaterialComponent>)>();
    for (_, (t, mesh, m)) in mesh_query.iter() {
        items.push(InterleavedItem::Mesh {
            g: crate::ecs::sorting_group::SortGroup::NONE,
            z: mesh.z_order,
            tex: &mesh.texture_id,
            mat: m.map(|m| m.material_id.as_str()),
            params: boxed_params(material_param_bits(m)),
            blend: sprite_blend(m),
            cutout: false,
            mask: 0,
            transform: WorldTransform::from(t),
            mesh,
        });
    }
    let mut path_query = world.query::<(&Transform, &VectorPath)>();
    for (_, (t, vp)) in path_query.iter() {
        items.push(InterleavedItem::Path {
            g: crate::ecs::sorting_group::SortGroup::NONE,
            z: vp.z_order,
            blend: crate::renderer::material::BlendMode::Alpha,
            cutout: false,
            mask: 0,
            transform: WorldTransform::from(t),
            parts: Vec::new(),
        });
    }
    let mut shadow_query = world.query::<(&Transform, &SpriteComponent, &ShadowCaster)>();
    for (_, (transform, sprite, shadow)) in shadow_query.iter() {
        if !shadow.enabled {
            continue;
        }
        items.push(InterleavedItem::Shadow {
            g: crate::ecs::sorting_group::SortGroup::NONE,
            z: sprite.z_order,
            tex: &sprite.texture_id,
            pos: transform.position,
            size: Vec2::ONE,
            rot: 0.0,
            skew: shadow.skew,
            color: Vec4::from(shadow.color),
            uv: [0.0, 0.0, 1.0, 1.0],
            anchor: sprite.anchor,
        });
        let _ = default_size; // size unused in the key-only collection
    }
    items.sort_by(|a, b| {
        a.sort_key()
            .cmp(&b.sort_key())
            .then_with(|| a.params().cmp(&b.params()))
            .then_with(|| blend_rank(a.blend()).cmp(&blend_rank(b.blend())))
    });
    items
        .into_iter()
        .map(|it| match it {
            InterleavedItem::Shadow { z, tex, .. } => (z, 0u8, tex.to_string(), None),
            InterleavedItem::Sprite { z, tex, mat, .. } => {
                (z, 1u8, tex.to_string(), mat.map(str::to_string))
            }
            InterleavedItem::Text { z, tex, .. } => (z, 2u8, tex.to_string(), None),
            InterleavedItem::Mesh { z, tex, mat, .. } => (z, 1u8, tex.to_string(), mat.map(str::to_string)),
            InterleavedItem::Path { z, .. } => (z, 1u8, crate::asset::WHITE_TEXTURE_ID.to_string(), None),
            InterleavedItem::Skinned { z, tex, mat, .. } => (z, 1u8, tex.to_string(), mat.map(str::to_string)),
            InterleavedItem::Tile { z, tex, .. } => (z, 1u8, tex.to_string(), None),
            InterleavedItem::Icon { z, tex, .. } => (z, 2u8, tex.to_string(), None),
            InterleavedItem::Gpu { z, tex, .. } => (z, 3u8, tex.to_string(), None),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every drawable of the pass is one of these, ten thousand to a frame of
    /// a crowd: a large variant makes all of them large (`Gpu`'s uniform was
    /// once inline, and every item was ~640 bytes).
    #[test]
    fn interleaved_items_stay_small() {
        let size = std::mem::size_of::<InterleavedItem<'static>>();
        eprintln!("InterleavedItem: {size} bytes");
        assert!(size <= 176, "InterleavedItem is {size} bytes - box the largest variant's payload");
    }

    // ── Entity-UI widget geometry ────────────────────────────────────────
    // A progress bar with the scene-40 range 0..100 and a slider on 0..1
    // must agree on where the same fill sits.

    #[test]
    fn widget_fraction_maps_the_widgets_own_range() {
        assert_eq!(fraction(35.0, 0.0, 100.0), 0.35);
        assert_eq!(fraction(0.35, 0.0, 1.0), 0.35);
        assert_eq!(fraction(150.0, 0.0, 100.0), 1.0, "over max clamps");
        assert_eq!(fraction(-5.0, 0.0, 100.0), 0.0, "under min clamps");
        assert_eq!(fraction(7.0, 3.0, 3.0), 0.0, "an empty range must not divide by zero");
    }

    #[test]
    fn a_disabled_button_is_dimmed_whatever_its_state() {
        let btn = UIButton { pressed: true, hovered: true, disabled: true, ..Default::default() };
        let c = button_display_color(&btn);
        assert_eq!(c, dimmed(btn.normal_color));
        let btn = UIButton { pressed: true, hovered: true, ..Default::default() };
        assert_eq!(button_display_color(&btn), btn.pressed_color, "pressed wins over hovered");
    }

    /// "Half brightness" is half of what the eye sees, so it is taken in
    /// sRGB: a label at sRGB 0.95 dims to 0.475, where halving its linear
    /// value drew it at 0.70 and a disabled button read as enabled.
    #[test]
    fn disabled_dims_to_half_the_brightness_seen() {
        let to_linear = |s: f32| if s <= 0.040_45 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) };
        let to_srgb = |v: f32| if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        for s in [0.95_f32, 0.6, 0.3, 0.02] {
            let d = dimmed([to_linear(s), to_linear(s), to_linear(s), 0.7]);
            assert!((to_srgb(d[0]) - s * 0.5).abs() < 1e-4, "sRGB {s} dimmed to {}", to_srgb(d[0]));
            assert_eq!(d[3], 0.7, "alpha is kept");
        }
    }

    // ── Text centring ─────────────────────────────────────────
    // Every UI label - entity UI, GUI text, button, text input - places its
    // glyphs from one origin, and that origin is the top of the line.

    fn test_font(line_height: f32, pixel_size: f32) -> FontData {
        FontData {
            atlas_width: 64,
            atlas_height: 64,
            line_height,
            pixel_size,
            atlas_scale: 1.0,
            sdf: false,
            sdf_spread: 0.0,
            glyphs: Vec::new(),
            shaping: None,
        }
    }

    #[test]
    fn a_label_is_centred_in_its_box_not_hung_below_it() {
        // A 14px label in a 24px-tall button, the shape BUGS #60 was seen in.
        let font = test_font(20.0, 16.0);
        let scale = 14.0 / font.pixel_size;
        let (bottom, top_edge) = (0.0f32, 24.0f32);
        let center_y = (bottom + top_edge) / 2.0;

        let line_top = centered_line_top(&font, "Start", scale, center_y);
        let line_bottom = line_top - font.line_height * scale;
        assert!(line_top <= top_edge, "text starts above the box: {line_top}");
        assert!(line_bottom >= bottom, "text hangs out of the box: {line_bottom}");
        // Centred means equal slack above and below.
        assert!((top_edge - line_top - (line_bottom - bottom)).abs() < 1e-5);

        // What the paths did before: the origin *was* the centre, so the whole
        // line hung below it - out of the bottom of any box this size.
        assert!(center_y - font.line_height * scale < bottom);
    }

    #[test]
    fn a_multi_line_label_centres_the_whole_block() {
        let font = test_font(20.0, 20.0);
        let one = centered_line_top(&font, "one", 1.0, 0.0);
        let two = centered_line_top(&font, "one\ntwo", 1.0, 0.0);
        assert_eq!(one, 10.0);
        assert_eq!(two, 20.0, "two lines start a full line higher");
        // A trailing newline is not a second line - `layout_text` would not
        // draw one, and counting it would shift the text up by half a line.
        assert_eq!(centered_line_top(&font, "one\n", 1.0, 0.0), one);
    }

    // ── GUI draw coalescing ──────────────────────────────────────────────
    // `UiRun::continues` decides whether a draw joins the open batch. Getting
    // it wrong does not fail loudly: draws merge into a flush that binds the
    // wrong texture and the frame renders subtly incorrectly.

    fn run_with(cur: Option<UiTarget>) -> UiRun {
        UiRun { cur, flushes: 0 }
    }

    #[test]
    fn ui_run_merges_consecutive_white_quads() {
        // Panel backgrounds, borders and button chrome all bind the white
        // fallback — this is the case that turns 110 draws into a handful.
        let r = run_with(Some(UiTarget::White));
        assert!(r.continues(&UiTarget::White));
    }

    #[test]
    fn ui_run_never_merges_different_bindings() {
        let white = run_with(Some(UiTarget::White));
        assert!(!white.continues(&UiTarget::Image("a.png".into())));
        assert!(!white.continues(&UiTarget::Font("f".into())));

        // Two different images must not share a flush: the batch can only
        // bind one texture, so the second would draw with the first's.
        let img = run_with(Some(UiTarget::Image("a.png".into())));
        assert!(img.continues(&UiTarget::Image("a.png".into())));
        assert!(!img.continues(&UiTarget::Image("b.png".into())));
        assert!(!img.continues(&UiTarget::White));

        // Same for font atlases, which are just as texture-bound.
        let font = run_with(Some(UiTarget::Font("built_in".into())));
        assert!(font.continues(&UiTarget::Font("built_in".into())));
        assert!(!font.continues(&UiTarget::Font("ui.font".into())));
    }

    #[test]
    fn ui_run_starts_closed() {
        // Nothing is open before the first draw, so the first `want` must
        // begin a batch rather than append to a phantom one.
        let r = run_with(None);
        assert!(!r.continues(&UiTarget::White));
        assert!(!r.continues(&UiTarget::Image("a.png".into())));
    }

    /// Collect the (size, uv, anchor) tuples a sprite would emit so we can
    /// inspect the geometry the renderer pushes downstream — without needing
    /// a live wgpu device.
    fn collect_quads(sprite: &SpriteComponent, default: Vec2, scale: Vec2) -> Vec<(Vec2, [f32; 4], [f32; 2])> {
        let mut out = Vec::new();
        for_each_fill_quad(sprite, default, scale, |s, uv, a| out.push((s, uv, a)));
        out
    }

    /// Recover the local-space rect a sub-quad covers (mirrors the inverse of
    /// [`rect_to_size_anchor`]) so we can assert on cell positions.
    fn rect_of(size: Vec2, anchor: [f32; 2]) -> (f32, f32, f32, f32) {
        // batch corners go (-ax*sw, -ay*sh) … ((1-ax)*sw, (1-ay)*sh)
        let x0 = -anchor[0] * size.x;
        let x1 = (1.0 - anchor[0]) * size.x;
        let y0 = -anchor[1] * size.y;
        let y1 = (1.0 - anchor[1]) * size.y;
        (x0, x1, y0, y1)
    }

    #[test]
    fn stretch_emits_single_quad_matching_legacy_path() {
        let sprite = SpriteComponent {
            size: Some([100.0, 50.0]),
            anchor: [0.5, 0.5],
            ..Default::default()
        };
        let quads = collect_quads(&sprite, Vec2::ONE, Vec2::ONE);
        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0].0, Vec2::new(100.0, 50.0));
        assert_eq!(quads[0].1, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(quads[0].2, [0.5, 0.5]);
    }

    #[test]
    fn none_fill_matches_stretch() {
        let mut sprite = SpriteComponent {
            size: Some([10.0, 20.0]),
            ..Default::default()
        };
        let with_none = collect_quads(&sprite, Vec2::ONE, Vec2::ONE);
        sprite.fill = Some(SpriteFill::Stretch);
        let with_stretch = collect_quads(&sprite, Vec2::ONE, Vec2::ONE);
        assert_eq!(with_none, with_stretch);
    }

    #[test]
    fn nine_slice_emits_nine_cells_tiling_the_sprite() {
        let sprite = SpriteComponent {
            size: Some([100.0, 60.0]),
            anchor: [0.0, 0.0], // origin at bottom-left for easy math
            fill: Some(SpriteFill::NineSlice { left: 10.0, right: 20.0, top: 5.0, bottom: 15.0, source_px: None }),
            ..Default::default()
        };
        let quads = collect_quads(&sprite, Vec2::ONE, Vec2::ONE);
        assert_eq!(quads.len(), 9);

        // Total covered area must equal sprite rect [0,100] x [0,60].
        let mut total_area = 0.0_f32;
        for (size, _uv, anchor) in &quads {
            let (x0, x1, y0, y1) = rect_of(*size, *anchor);
            assert!(x0 >= -1e-3 && x1 <= 100.0 + 1e-3, "cell x out of bounds: {x0}..{x1}");
            assert!(y0 >= -1e-3 && y1 <= 60.0 + 1e-3, "cell y out of bounds: {y0}..{y1}");
            total_area += (x1 - x0) * (y1 - y0);
        }
        assert!((total_area - 100.0 * 60.0).abs() < 1e-2, "cells must tile sprite: {total_area}");

        // With no texture size resolved yet, corner UVs are the border's
        // fraction of the *drawn* size - the stretch-equivalent fallback.
        // Find top-left cell (smallest x, largest y).
        let (tl_size, tl_uv, tl_anchor) = quads.iter()
            .copied()
            .min_by(|a, b| {
                let (_, _, _, ay1) = rect_of(a.0, a.2);
                let (_, _, _, by1) = rect_of(b.0, b.2);
                let (ax0, _, _, _) = rect_of(a.0, a.2);
                let (bx0, _, _, _) = rect_of(b.0, b.2);
                // top-left = small x AND large y → score = x - y
                (ax0 - ay1).partial_cmp(&(bx0 - by1)).unwrap()
            })
            .unwrap();
        let _ = tl_anchor;
        assert!((tl_size.x - 10.0).abs() < 1e-3);
        assert!((tl_size.y - 5.0).abs() < 1e-3);
        // u: 0..left/W = 0..0.1; v: 0..top/H = 0..(5/60)
        assert!((tl_uv[0] - 0.0).abs() < 1e-3);
        assert!((tl_uv[2] - 0.1).abs() < 1e-3);
        assert!((tl_uv[1] - 0.0).abs() < 1e-3);
        assert!((tl_uv[3] - (5.0 / 60.0)).abs() < 1e-3);
    }

    /// The point of nine-slice: a corner samples the texture's own corner.
    ///
    /// With a 48 px texture and 16-unit borders, the top-left cell must be
    /// 16x16 on screen *and* sample the texture's top-left third - at any
    /// drawn size. Before BUGS #96 the UV was `border / drawn size` (16/240
    /// here, 0.067), which is exactly what a stretch samples.
    #[test]
    fn nine_slice_corners_sample_the_texture_corners() {
        for size in [[240.0, 96.0], [64.0, 64.0]] {
            let sprite = SpriteComponent {
                size: Some(size),
                anchor: [0.0, 0.0],
                fill: Some(SpriteFill::NineSlice {
                    left: 16.0,
                    right: 16.0,
                    top: 16.0,
                    bottom: 16.0,
                    source_px: Some([48.0, 48.0]),
                }),
                ..Default::default()
            };
            let quads = collect_quads(&sprite, Vec2::ONE, Vec2::ONE);
            assert_eq!(quads.len(), 9);
            let (tl_size, tl_uv, _) = quads
                .iter()
                .copied()
                .min_by(|a, b| {
                    let (ax0, _, _, ay1) = rect_of(a.0, a.2);
                    let (bx0, _, _, by1) = rect_of(b.0, b.2);
                    (ax0 - ay1).partial_cmp(&(bx0 - by1)).unwrap()
                })
                .unwrap();
            assert!((tl_size.x - 16.0).abs() < 1e-3 && (tl_size.y - 16.0).abs() < 1e-3);
            let third = 1.0 / 3.0;
            assert!(
                (tl_uv[2] - third).abs() < 1e-3 && (tl_uv[3] - third).abs() < 1e-3,
                "drawn {size:?}: the corner sampled up to {:?}, not the texture's outer third",
                [tl_uv[2], tl_uv[3]]
            );
        }
    }

    #[test]
    fn nine_slice_clamps_oversized_borders() {
        // Borders larger than sprite size should clamp, not panic / overflow.
        let sprite = SpriteComponent {
            size: Some([20.0, 20.0]),
            anchor: [0.0, 0.0],
            fill: Some(SpriteFill::NineSlice { left: 30.0, right: 30.0, top: 30.0, bottom: 30.0, source_px: None }),
            ..Default::default()
        };
        let quads = collect_quads(&sprite, Vec2::ONE, Vec2::ONE);
        // Middle row/col collapse to zero and are skipped → only 4 corner cells remain.
        assert_eq!(quads.len(), 4);
        let mut total = 0.0;
        for (size, _, _) in &quads {
            total += size.x * size.y;
        }
        assert!((total - 400.0).abs() < 1e-2);
    }

    #[test]
    fn tile_emits_grid_with_clipped_edges() {
        // 50x30 sprite, 20x20 tiles → 3 cols (last clipped to 10), 2 rows (last clipped to 10).
        let sprite = SpriteComponent {
            size: Some([50.0, 30.0]),
            anchor: [0.0, 0.0],
            fill: Some(SpriteFill::Tile { tile_size: [20.0, 20.0] }),
            ..Default::default()
        };
        let quads = collect_quads(&sprite, Vec2::ONE, Vec2::ONE);
        assert_eq!(quads.len(), 6);
        let mut area = 0.0;
        for (size, uv, _) in &quads {
            area += size.x * size.y;
            // UV right edge should equal width/tile_size fraction.
            let expected_u_max = size.x / 20.0;
            assert!((uv[2] - expected_u_max).abs() < 1e-3, "uv x clipped: {uv:?} size {size:?}");
            let expected_v_max = size.y / 20.0;
            assert!((uv[3] - expected_v_max).abs() < 1e-3, "uv y clipped: {uv:?} size {size:?}");
        }
        assert!((area - 50.0 * 30.0).abs() < 1e-2);
    }

    #[test]
    fn tile_with_invalid_size_falls_back_to_stretch() {
        let sprite = SpriteComponent {
            size: Some([100.0, 100.0]),
            fill: Some(SpriteFill::Tile { tile_size: [0.0, 0.0] }),
            ..Default::default()
        };
        let quads = collect_quads(&sprite, Vec2::ONE, Vec2::ONE);
        assert_eq!(quads.len(), 1);
        assert_eq!(quads[0].0, Vec2::new(100.0, 100.0));
    }

    /// Regression guard for the shadow-interleave fix: with the *old*
    /// single-monolithic-shadow-pass design, a backdrop sprite at z=-10
    /// painted on top of all shadows (drawn pre-sprite-pass), making
    /// shadows invisible against any backdrop. The interleave path must
    /// sort items so that:
    ///   1. Backdrop sprite (z=-10) renders first.
    ///   2. Caster's shadow (z=1, kind=shadow) renders next — on top of
    ///      the backdrop, where the multiply pipeline can darken it.
    ///   3. Caster sprite (z=1, kind=sprite) renders last, sitting on top
    ///      of its own shadow.
    ///
    /// Encoded as the bucket-key tuple `(z, kind, tex, mat)` returned by
    /// [`collect_interleaved_sort_keys`] — pure data, no GPU needed.
    #[test]
    fn interleave_puts_shadow_between_backdrop_and_caster_at_caster_z() {
        let mut world = World::new();

        // Backdrop at z = -10. No ShadowCaster — backdrops don't cast.
        world.spawn((
            Transform::default(),
            SpriteComponent {
                texture_id: "backdrop.png".into(),
                z_order: -10,
                ..Default::default()
            },
        ));

        // Caster sprite at z = 1 with a ShadowCaster.
        world.spawn((
            Transform::default(),
            SpriteComponent {
                texture_id: "caster.png".into(),
                z_order: 1,
                ..Default::default()
            },
            ShadowCaster::default(),
        ));

        let keys = collect_interleaved_sort_keys(&world, Vec2::splat(64.0));
        // Render order from back to front:
        let expected: Vec<(i32, u8, String, Option<String>)> = vec![
            (-10, 1, "backdrop.png".into(), None),
            (1, 0, "caster.png".into(), None), // shadow at caster z, BEFORE caster
            (1, 1, "caster.png".into(), None), // caster sprite on top
        ];
        assert_eq!(keys, expected, "interleave order broke — scene 31 shadows would vanish behind the backdrop again");
    }

    /// At the same `z_order`, shadow must precede sprite so a sprite
    /// covering its own shadow doesn't *hide* it. Tightens the contract
    /// above with a minimal two-item case (no backdrop noise).
    /// A mesh is a sprite as far as the sort is concerned: between the
    /// sprites below and above it by z, and in the same bucket as a sprite
    /// of its texture at its z. That is what makes it drawable *under* a
    /// sprite at all - `SkinnedMesh` and `VectorPath`, each its own pass
    /// after every sprite, cannot be.
    #[test]
    fn a_mesh_sorts_among_sprites_by_z() {
        let mut world = World::new();
        world.spawn((
            Transform::default(),
            SpriteComponent { texture_id: "tex.png".into(), z_order: 1, ..Default::default() },
        ));
        world.spawn((
            Transform::default(),
            SpriteComponent { texture_id: "tex.png".into(), z_order: 9, ..Default::default() },
        ));
        let mut mesh = Mesh2D::quad("tex.png", [[-1.0, 1.0], [1.0, 1.0], [1.0, -1.0], [-1.0, -1.0]]);
        mesh.z_order = 5;
        world.spawn((Transform::default(), mesh));
        let keys = collect_interleaved_sort_keys(&world, Vec2::splat(64.0));
        let zs: Vec<i32> = keys.iter().map(|k| k.0).collect();
        assert_eq!(zs, vec![1, 5, 9]);
        assert!(keys.iter().all(|k| k.1 == 1), "a mesh sorts as a sprite, not as its own kind: {keys:?}");
    }

    #[test]
    fn shadow_sorts_before_sprite_at_equal_z() {
        let mut world = World::new();
        world.spawn((
            Transform::default(),
            SpriteComponent {
                texture_id: "tex.png".into(),
                z_order: 5,
                ..Default::default()
            },
            ShadowCaster::default(),
        ));
        let keys = collect_interleaved_sort_keys(&world, Vec2::splat(64.0));
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].1, 0, "shadow (kind=0) must come first at equal z");
        assert_eq!(keys[1].1, 1, "sprite (kind=1) must come second at equal z");
    }

    /// Disabled `ShadowCaster` entities are skipped — same semantics as
    /// the standalone `render_shadows_wgpu` path. Without this the
    /// interleaver would emit useless empty-draw shadow flushes.
    #[test]
    fn disabled_shadow_caster_is_skipped_in_interleave() {
        let mut world = World::new();
        world.spawn((
            Transform::default(),
            SpriteComponent {
                texture_id: "tex.png".into(),
                z_order: 0,
                ..Default::default()
            },
            ShadowCaster {
                enabled: false,
                ..Default::default()
            },
        ));
        let keys = collect_interleaved_sort_keys(&world, Vec2::splat(64.0));
        assert_eq!(keys.len(), 1, "disabled shadow must not be emitted");
        assert_eq!(keys[0].1, 1, "only the sprite remains");
    }

    /// A normal-mapped (`lit_sprite`) sprite sorts with every other sprite.
    ///
    /// It used to be skipped here and drawn by a forward pass of its own
    /// after every sprite, so a normal-mapped floor covered whatever stood
    /// on it whatever the z orders said. Its normal is drawn by
    /// `render_normals_wgpu` now; its colour is drawn here, in z order.
    #[test]
    fn a_lit_sprite_sorts_with_the_other_sprites() {
        let mut world = World::new();
        world.spawn((
            Transform::default(),
            SpriteComponent { texture_id: "floor.png".into(), z_order: 0, ..Default::default() },
            MaterialComponent {
                material_id: LIT_SPRITE_MATERIAL_ID.into(),
                normal_map_id: Some("floor_n.png".into()),
                ..Default::default()
            },
        ));
        world.spawn((
            Transform::default(),
            SpriteComponent { texture_id: "crate.png".into(), z_order: 5, ..Default::default() },
        ));
        let keys = collect_interleaved_sort_keys(&world, Vec2::splat(64.0));
        let order: Vec<&str> = keys.iter().map(|k| k.2.as_str()).collect();
        assert_eq!(order, vec!["floor.png", "crate.png"], "the crate at z 5 draws over the floor at z 0");
    }
}

