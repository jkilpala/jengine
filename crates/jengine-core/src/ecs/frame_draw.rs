//! One frame's drawing, from the views to the UI - what native, the editor
//! viewport and the web runtime each carried a copy of (`docs/architecture-debt.md`,
//! #2). Every camera that draws into a texture, then the screen: the world in
//! parts (one per band of `SceneColor` readers), its normals, lights,
//! lightmaps, unlit emission and composite, the readers over a copy of the
//! scene, the post chain; then the UI on the display - entity UI, `.gui`
//! documents, touch controls and the debug overlay.
//!
//! The copies differed in policy, not in drawing, and policy is what a host
//! lends through [`DrawHost`]: how it finds a material's graph pipeline
//! (native reads the `.material` from disk, the web fetches it, the editor
//! asks for it) and what it says about a texture it does not have yet (each
//! host loads missing textures its own way, keyed off the `warned` set this
//! module fills). Everything else is here once.

use std::collections::{HashMap, HashSet};

use hecs::World;

use crate::asset::AssetManager;
use crate::asset::sort_layers::SortLayers;
use crate::debug_draw::DebugDrawCommand;
use crate::ecs::components::GuiComponent;
use crate::ecs::wgpu_systems::{
    self, SpriteShadowFlush, UiFlush, ViewTargets, WorldLayer, render_gui_wgpu, render_particles_in_layer,
    render_tilemaps_wgpu, render_ui_wgpu,
};
use crate::renderer::backend::wgpu::{
    WgpuFrame, WgpuLightingPass, WgpuLitSpritePipeline, WgpuPostProcessStack, WgpuSpriteBatch, WgpuTexture,
};
use crate::renderer::camera::Camera2D;
use crate::renderer::font_asset::FrameFonts;
use crate::renderer::graph_pipeline_cache::{GraphPipelineCache, material_reads_scene};
use crate::ui::gui_document::GuiDocument;

/// What a host decides for the frame's draw: its policy, where the hosts
/// differ for good reasons.
pub trait DrawHost {
    /// The pipeline material `mid`'s shader graph draws with, built on first
    /// use, or `None` for the default sprite pipeline - the material is not
    /// loaded (yet), names no graph, or its graph is not ready.
    #[allow(clippy::too_many_arguments)]
    fn material_pipeline(
        &mut self,
        assets: &mut AssetManager,
        graph_pipelines: &mut GraphPipelineCache,
        warned: &mut HashSet<String>,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        batch: &WgpuSpriteBatch,
        mid: &str,
    ) -> Option<std::sync::Arc<wgpu::RenderPipeline>>;

    /// A world texture with no pixels yet, met for the first time: the host
    /// says what it will do about it (its loader reads the `warned` set).
    fn texture_missing(&mut self, tex_id: &str);

    /// Something worth a warning, once (the caller keeps it to once).
    fn warn(&mut self, message: &str);
}

/// What the frame draws with: the host's GPU objects and assets, lent for
/// the frame.
pub struct FrameDraw<'a, 'f> {
    pub device: &'a wgpu::Device,
    pub queue: &'a wgpu::Queue,
    /// The world's format (a float target where the adapter allows).
    pub scene_format: wgpu::TextureFormat,
    /// The display's format, for the UI.
    pub color_format: wgpu::TextureFormat,
    pub world: &'a World,
    pub assets: &'a mut AssetManager,
    /// Ids already warned about - also how a host's loader learns what is
    /// missing: a world texture with no pixels is recorded under its id.
    pub warned: &'a mut HashSet<String>,
    pub graph_pipelines: &'a mut GraphPipelineCache,
    pub sprite_batch: &'a mut WgpuSpriteBatch,
    pub lit_sprite_batch: &'a mut WgpuLitSpritePipeline,
    pub flat_normal: &'a WgpuTexture,
    pub particle_system: &'a crate::particles::ParticleSystem,
    pub frame_fonts: &'a FrameFonts<'f>,
    pub sort_layers: &'a SortLayers,
    /// The screen's post chain and lighting; each view has its own.
    pub post: &'a mut WgpuPostProcessStack,
    pub light: &'a mut WgpuLightingPass,
    pub view_targets: &'a mut HashMap<String, ViewTargets>,
    pub host: &'a mut dyn DrawHost,
}

/// What the world pass shows.
pub struct WorldScene<'a> {
    /// The main camera, after `apply_pixel_plan`.
    pub camera: Camera2D,
    /// The surface in pixels.
    pub surface: (u32, u32),
    /// Whether the lit path runs (`has_lights`, and the editor's toggle).
    pub lighting: bool,
    pub clear: wgpu::Color,
    /// Script `draw_debug_*` shapes, drawn in the world over everything.
    pub debug_draws: &'a [DebugDrawCommand],
    /// The F4 collider and joint outlines.
    pub physics_outlines: bool,
    /// Drawn first into the screen's world pass (the editor's grid).
    pub underlay: Option<&'a mut dyn FnMut(&mut wgpu::RenderPass<'_>, &Camera2D)>,
}

/// What the UI pass shows.
pub struct UiScene<'a> {
    pub ui_scale: crate::ui::scaling::UiScale,
    pub gui_jobs: &'a [(GuiDocument, GuiComponent)],
    pub touch_draws: &'a [crate::input::touch_controls::TouchDraw],
    /// The F3 / F12 overlay, when shown.
    pub overlay: Option<(&'a crate::debug_overlay::DebugOverlayState, &'a crate::profiling::FrameStats)>,
}

/// Draw the frame: every view, the screen's world and post chain, then the
/// UI. The host has begun the frame and presents it afterwards (with its fade
/// overlay, which goes over the UI).
pub fn draw_frame(d: FrameDraw<'_, '_>, frame: &mut WgpuFrame, scene: WorldScene<'_>, ui: UiScene<'_>) {
    let FrameDraw {
        device,
        queue,
        scene_format,
        color_format,
        world,
        assets,
        warned,
        graph_pipelines,
        sprite_batch,
        lit_sprite_batch,
        flat_normal,
        particle_system,
        frame_fonts,
        sort_layers,
        post: screen_post,
        light: screen_light,
        view_targets,
        host,
    } = d;
    let WorldScene { camera: main_camera, surface, lighting, clear: scene_clear, debug_draws, physics_outlines, mut underlay } =
        scene;
    let (vw, vh) = surface;
    // Where the time goes (`profiling::trace`): a span per pass, each with
    // the draws made inside it.
    use crate::profiling::trace::span;
    let _render = span("render");

    // Once a frame, before the views: `begin_frame` rewinds the vertex arena,
    // and every pass's vertices are written before the encoder runs - begun
    // per view, the last pass's normals overwrote every earlier view's.
    lit_sprite_batch.begin_frame();
    sprite_batch.set_target_format(device, scene_format);
    // Stateful GPU particles (`particles::gpu_state`): stepped once a frame,
    // before any view draws them.
    {
        let _s = span("particles.state");
        let emitters = crate::particles::gpu_state::gather(world);
        sprite_batch.step_particle_states(device, queue, &mut frame.encoder, &emitters);
    }

    // With lighting, the world pass is the scene buffer's layer (unlit things
    // as cutouts); the emission pass draws them.
    let world_layer = if lighting { WorldLayer::Scene } else { WorldLayer::All };

    // Views (`ecs::views`): cameras that draw into textures, drawn first so
    // the main camera's sprites show this frame's picture - and canvas groups
    // (`ecs::canvas_group`), a view each through the main camera, drawing
    // only the group's members onto transparent.
    let mut views = crate::ecs::views::render_target_cameras(world, (vw, vh));
    views.extend(crate::ecs::canvas_group::canvas_group_views(world, &main_camera, (vw, vh)));
    // Off-screen markers (`ecs::offscreen_marker`) are placed from the
    // screen's view as it was last drawn.
    crate::ecs::views::note_main_view(&main_camera);
    // Mirrors (`ecs::mirror`): a view each of what is in front of the line.
    #[cfg(feature = "mirror")]
    views.extend(crate::ecs::mirror::mirror_views(world, &main_camera));
    // Portals (`ecs::portal`): a view each of what is in front of the partner.
    #[cfg(feature = "portal")]
    views.extend(crate::ecs::portal::portal_views(world, &main_camera));
    if !views.is_empty() || !view_targets.is_empty() {
        let settings = screen_post.settings().clone();
        wgpu_systems::sync_view_targets(view_targets, &views, device, assets, scene_format, &settings);
    }
    // The world, in parts when a `SceneColor` sprite has something in front of
    // it: up to each band of readers, then a copy and the readers, then the
    // rest over them (`wgpu_systems::ZWindow`). A lit frame draws a later part
    // into a transparent scene buffer and composites it over; one pass when
    // nothing stands in front of a reader.
    // Post volumes (`ecs::post_volume`): the screen's chain blended by
    // where the main camera is, the stack's own settings untouched.
    #[cfg(feature = "post_volume")]
    screen_post.set_blended(crate::ecs::post_volume::blended(world, screen_post.settings(), main_camera.position));
    let plan = wgpu_systems::SceneReaderPlan::of(world, sort_layers);

    for which in (0..views.len()).map(Some).chain([None]) {
        let _camera_span = span(if which.is_some() { "render.view" } else { "render.world" });
        // A canvas group's picture starts transparent, to go over the world.
        let clear = if which.is_some_and(|v| views[v].transparent) { wgpu::Color::TRANSPARENT } else { scene_clear };
        let (camera, target, post, light) = match which {
            Some(v) => {
                let view = &views[v];
                let Some(t) = view_targets.get_mut(&view.target) else { continue };
                let cam = wgpu_systems::apply_pixel_plan(view.camera, view.size, device, &mut t.post, &mut t.light);
                (cam, Some(view.target.as_str()), &mut t.post, &mut t.light)
            }
            None => (main_camera, None, &mut *screen_post, &mut *screen_light),
        };
        let view_projection = camera.view_projection();
        crate::renderer::frame_view::set_frame_view(&camera);
        sprite_batch.set_target_format(device, scene_format);
        let mut scene_skipped = wgpu_systems::has_scene_readers(world);

        // The world's drawables, collected once for this camera - culled,
        // built and their layers noted - and drawn by every layer and band
        // below: the scene layer, the emission layer and the readers'. A lit
        // frame collected the world twice or more until Oct 2026
        // (architecture-debt #9).
        wgpu_systems::render_world_items(world, sort_layers, frame_fonts, glam::Vec2::splat(64.0), |collected| {
            for part in 0..plan.parts() {
                let above = part > 0;
                wgpu_systems::set_z_window(plan.world_window(part));
                let scene_target = if lighting { light.scene_target() } else { post.scene_target() };
                let mut pass = match (above, lighting) {
                    (false, _) => scene_target.begin_pass(&mut frame.encoder, Some(clear)),
                    (true, true) => light.overlay_scene_pass(&mut frame.encoder),
                    (true, false) => scene_target.begin_pass(&mut frame.encoder, None),
                };
                if !above
                    && target.is_none()
                    && let Some(draw) = underlay.as_mut()
                {
                    draw(&mut pass, &camera);
                }

                // The world's draws share one flush: a texture uploaded or the
                // draw dropped, a material's graph pipeline, shadows multiplied.
                // One closure for sprites and shadows alike, so the `&mut`
                // captures of the assets and the pass stay legal.
                {
                    let mut flush_dispatch = |batch: &mut WgpuSpriteBatch, tex_id: &str, kind: SpriteShadowFlush<'_>| {
                        if assets.get_wgpu_texture(tex_id).is_none() {
                            if !assets.has_texture_bytes(tex_id) {
                                if warned.insert(tex_id.to_string()) {
                                    host.texture_missing(tex_id);
                                }
                                // Queued vertices must not bleed into the next
                                // flush against an unrelated texture.
                                batch.discard_pending();
                                return;
                            }
                            if let Err(e) = assets.ensure_wgpu_texture(tex_id, device, queue) {
                                if warned.insert(tex_id.to_string()) {
                                    host.warn(&format!("upload failed for '{tex_id}': {e:#}"));
                                }
                                batch.discard_pending();
                                return;
                            }
                        }
                        let material_id = match kind {
                            SpriteShadowFlush::Shadow => {
                                let tex = assets.get_wgpu_texture(tex_id).expect("just ensured");
                                batch.flush_multiply(device, queue, &mut pass, &view_projection, tex);
                                return;
                            }
                            SpriteShadowFlush::Sprite { material_id } => material_id,
                        };
                        let pipeline = material_id.and_then(|mid| {
                            host.material_pipeline(assets, graph_pipelines, warned, device, scene_format, batch, mid)
                        });
                        // A graph that reads the scene draws after the world, over a
                        // copy of it - the readers' pass below.
                        if let Some(mid) = material_id
                            && material_reads_scene(assets, graph_pipelines, mid)
                        {
                            scene_skipped = true;
                            batch.discard_pending();
                            return;
                        }
                        let tex = assets.get_wgpu_texture(tex_id).expect("just ensured");
                        match pipeline {
                            Some(p) => {
                                batch.flush_with_pipeline(device, queue, &mut pass, &view_projection, tex, &p);
                            }
                            None => {
                                batch.flush(device, queue, &mut pass, &view_projection, tex);
                            }
                        }
                    };
                    {
                        let _s = span("render.tilemaps");
                        render_tilemaps_wgpu(world, sort_layers, sprite_batch, &camera, &mut |b: &mut WgpuSpriteBatch, t: &str| {
                            flush_dispatch(b, t, SpriteShadowFlush::Sprite { material_id: None })
                        });
                    }
                    // Sprites, shadows, text, meshes, paths, lines, trails and
                    // skinned meshes, in one z order.
                    {
                        let _s = span("render.sprites");
                        collected.draw(world_layer, sprite_batch, &mut flush_dispatch);
                    }
                    // An untextured emitter draws against the 1x1 white.
                    let mut flush_particle = |batch: &mut WgpuSpriteBatch, tex_id: Option<&str>| {
                        flush_dispatch(batch, tex_id.unwrap_or("__white"), SpriteShadowFlush::Sprite { material_id: None });
                    };
                    {
                        let _s = span("render.particles");
                        render_particles_in_layer(world, sort_layers, particle_system, sprite_batch, world_layer, &mut flush_particle);
                    }
                    // Script debug shapes over the scene, then the F4 outlines.
                    let _debug = span("render.debug");
                    crate::debug_draw::render_debug_commands(debug_draws, sprite_batch, &mut flush_particle);
                    if physics_outlines {
                        let shapes = crate::debug_draw::physics_debug_commands(world);
                        crate::debug_draw::render_debug_commands(&shapes, sprite_batch, &mut flush_particle);
                    }
                }

                // The composite multiplies the scene by the light buffer; the
                // world pass ends here so it can run.
                drop(pass);
                if lighting {
                    if !above {
                        // Every normal-mapped sprite's normal (and a caster's
                        // mark, and its light layers) into the normal buffer, in
                        // draw order, before the lights read it. Skipped when
                        // nothing needs it; `render_lights` then clears it flat.
                        if wgpu_systems::needs_normal_pass(world) {
                            let _s = span("render.normals");
                            let mut np = light.normal_pass(&mut frame.encoder);
                            wgpu_systems::render_normals_wgpu(
                                world,
                                sort_layers,
                                lit_sprite_batch,
                                glam::Vec2::splat(64.0),
                                |pipeline, diffuse_id, normal_id| {
                                    match wgpu_systems::lit_sprite_textures(
                                        assets, device, queue, diffuse_id, normal_id, flat_normal,
                                    ) {
                                        Ok((diffuse, normal, missing_normal)) => {
                                            if let Some(nid) = missing_normal
                                                && warned.insert(format!("__normal:{nid}"))
                                            {
                                                host.warn(&format!("normal map '{nid}' is not loaded; shading {diffuse_id} flat"));
                                            }
                                            pipeline.flush_normals(device, queue, &mut np, &view_projection, diffuse, normal);
                                        }
                                        Err(why) => {
                                            if warned.insert(format!("__normals:{diffuse_id}")) {
                                                host.warn(&format!("no normals for {diffuse_id}: {why}"));
                                            }
                                        }
                                    }
                                },
                            );
                        }
                        {
                            let _s = span("render.lights");
                            let lights = crate::renderer::lighting::gather_lights(world);
                            // Each light's cookie, uploaded and bound (`light_cookie.wgsl`).
                            light.prepare_cookies(device, queue, &lights, assets);
                            light.render_lights(device, queue, &mut frame.encoder, &camera, &lights);
                        }
                        let _lightmaps = span("render.lightmaps");
                        // Baked lightmaps add into the light buffer before the
                        // composite, like a light (BUGS #95). One with a graph
                        // material is procedural light, drawn through the graph -
                        // and not at all until the graph is ready, since plain it
                        // is a white box of light.
                        let mut lp = light.light_buffer_pass(&mut frame.encoder);
                        wgpu_systems::render_lightmaps_wgpu(world, sprite_batch, |batch, tex_id, material| {
                            if assets.get_wgpu_texture(tex_id).is_none()
                                && (!assets.has_texture_bytes(tex_id) || assets.ensure_wgpu_texture(tex_id, device, queue).is_err())
                            {
                                if warned.insert(format!("__lightmap:{tex_id}")) {
                                    host.warn(&format!("lightmap texture '{tex_id}' is not loaded"));
                                }
                                batch.discard_pending();
                                return;
                            }
                            let pipeline = match material {
                                Some(mid) => {
                                    match host.material_pipeline(assets, graph_pipelines, warned, device, scene_format, batch, mid) {
                                        Some(p) => Some(p),
                                        None => {
                                            batch.discard_pending();
                                            return;
                                        }
                                    }
                                }
                                None => None,
                            };
                            let tex = assets.get_wgpu_texture(tex_id).expect("just ensured");
                            match pipeline {
                                Some(p) => {
                                    batch.flush_with_pipeline(device, queue, &mut lp, &view_projection, tex, &p);
                                }
                                None => {
                                    batch.flush_additive(device, queue, &mut lp, &view_projection, tex);
                                }
                            }
                        });
                    }
                    // `Unlit` things, in z order with the lit world into the
                    // emission buffer, cut out wherever something lit is in front,
                    // and added by the composite at full brightness.
                    if collected.draws_emission() {
                        let _s = span("render.emission");
                        let mut ep = light.emission_pass(&mut frame.encoder);
                        let emission = WorldLayer::Emission;
                        let mut emit =
                            |batch: &mut WgpuSpriteBatch, tex_id: &str, kind: SpriteShadowFlush<'_>| {
                                if assets.get_wgpu_texture(tex_id).is_none()
                                    && (!assets.has_texture_bytes(tex_id)
                                        || assets.ensure_wgpu_texture(tex_id, device, queue).is_err())
                                {
                                    batch.discard_pending();
                                    return;
                                }
                                // An unlit sprite's graph material draws here too.
                                let mid = match kind {
                                    SpriteShadowFlush::Sprite { material_id: Some(mid) } => Some(mid),
                                    _ => None,
                                };
                                let pipeline = mid.and_then(|mid| {
                                    host.material_pipeline(assets, graph_pipelines, warned, device, scene_format, batch, mid)
                                });
                                if let Some(mid) = mid
                                    && material_reads_scene(assets, graph_pipelines, mid)
                                {
                                    batch.discard_pending();
                                    return;
                                }
                                let tex = assets.get_wgpu_texture(tex_id).expect("just ensured");
                                match pipeline {
                                    Some(p) => {
                                        batch.flush_with_pipeline(device, queue, &mut ep, &view_projection, tex, &p);
                                    }
                                    None => {
                                        batch.flush(device, queue, &mut ep, &view_projection, tex);
                                    }
                                }
                            };
                        collected.draw(emission, sprite_batch, &mut emit);
                        // Unlit emitters, and lit ones as cutouts over them.
                        render_particles_in_layer(
                            world,
                            sort_layers,
                            particle_system,
                            sprite_batch,
                            emission,
                            |b: &mut WgpuSpriteBatch, t: Option<&str>| {
                                emit(b, t.unwrap_or("__white"), SpriteShadowFlush::Sprite { material_id: None })
                            },
                        );
                    }
                    let _composite = span("render.composite");
                    if above {
                        light.composite_over(&mut frame.encoder, &post.scene_target().view);
                    } else {
                        light.set_hdr(queue, post.hdr_active());
                        light.composite(&mut frame.encoder, &post.scene_target().view);
                    }
                }

                // Sprites whose graph reads the scene drawn under them
                // (`SceneColor` - heat haze, refraction), skipped by the world
                // pass: what is drawn so far is copied, after the composite, and
                // this part's band of readers is drawn over it - so a lit scene's
                // readers bend the lit image, glows and all. The next part draws
                // over them, and the next band copies them too.
                wgpu_systems::set_z_window(plan.readers_after(part));
                if scene_skipped && plan.readers_after(part).is_some() {
                    let _s = span("render.scene_readers");
                    let target = post.scene_target();
                    sprite_batch.capture_scene(device, &mut frame.encoder, &target.texture, (target.width, target.height), target.format);
                    let mut sp = target.begin_pass(&mut frame.encoder, None);
                    collected.draw(
                        WorldLayer::All,
                        sprite_batch,
                        |batch: &mut WgpuSpriteBatch, tex_id: &str, kind| {
                            let SpriteShadowFlush::Sprite { material_id: Some(mid) } = kind else {
                                batch.discard_pending();
                                return;
                            };
                            if !material_reads_scene(assets, graph_pipelines, mid)
                                || (assets.get_wgpu_texture(tex_id).is_none()
                                    && (!assets.has_texture_bytes(tex_id)
                                        || assets.ensure_wgpu_texture(tex_id, device, queue).is_err()))
                            {
                                batch.discard_pending();
                                return;
                            }
                            let Some(pipeline) =
                                host.material_pipeline(assets, graph_pipelines, warned, device, scene_format, batch, mid)
                            else {
                                batch.discard_pending();
                                return;
                            };
                            let tex = assets.get_wgpu_texture(tex_id).expect("just ensured");
                            batch.flush_with_pipeline(device, queue, &mut sp, &view_projection, tex, &pipeline);
                        },
                    );
                }
            }
        });
        wgpu_systems::set_z_window(None);

        // The velocity pass (`motion_blur`, `depth_of_field`): each sprite's
        // motion across the screen, over the camera's own, and its z, for
        // the main camera.
        if target.is_none() && (post.motion_blur_on() || post.depth_of_field_on() || post.depth_haze_on()) {
            let _s = span("render.velocity");
            let (camera_motion, scale) = wgpu_systems::begin_velocity(&camera);
            let pipeline = sprite_batch.velocity_pipeline(device);
            sprite_batch.set_target_format(device, crate::renderer::backend::wgpu::VELOCITY_FORMAT);
            {
                let clear = wgpu::Color {
                    r: camera_motion[0] as f64,
                    g: camera_motion[1] as f64,
                    b: post.depth_clear() as f64,
                    a: 0.0,
                };
                let mut vp = post.velocity_target().begin_pass(&mut frame.encoder, Some(clear));
                wgpu_systems::render_velocity_wgpu(
                    world,
                    sort_layers,
                    sprite_batch,
                    glam::Vec2::splat(64.0),
                    &camera,
                    scale,
                    |batch: &mut WgpuSpriteBatch, tex_id: &str| {
                        if assets.get_wgpu_texture(tex_id).is_none()
                            && (!assets.has_texture_bytes(tex_id) || assets.ensure_wgpu_texture(tex_id, device, queue).is_err())
                        {
                            batch.discard_pending();
                            return;
                        }
                        let tex = assets.get_wgpu_texture(tex_id).expect("just ensured");
                        batch.flush_with_pipeline(device, queue, &mut vp, &view_projection, tex, &pipeline);
                    },
                );
            }
            sprite_batch.set_target_format(device, scene_format);
        }

        // The post chain runs on the world and its output pass writes the
        // view's texture or the display; the UI is drawn after it, so bloom
        // and the vignette never touch it.
        let _post = span("render.post");
        match target {
            Some(id) => {
                if let Some(tex) = assets.get_wgpu_texture(id) {
                    post.execute(queue, &mut frame.encoder, &tex.view);
                }
            }
            None => post.execute(queue, &mut frame.encoder, &frame.view),
        }
    }

    // ── The UI, on the display ───────────────────────────────────────────
    let UiScene { ui_scale, gui_jobs, touch_draws, overlay } = ui;
    let _ui = span("render.ui");
    let mut pass = frame.overlay_pass();
    sprite_batch.set_target_format(device, color_format);
    // Laid out and drawn in virtual pixels; the projection scales it
    // (`ui::scaling`).
    let (uw, uh) = ui_scale.virtual_size(vw as f32, vh as f32);
    let controls_offset = ui_scale.controls_offset(vw as f32, vh as f32);
    let ui_vp = ui_scale.projection(vw as f32, vh as f32);
    let mut ui_flush = |batch: &mut WgpuSpriteBatch, what: UiFlush<'_>| {
        // A scissor change, not a draw: a `.gui` node inside an
        // `Overflow::Hidden` / `Scroll` container paints only inside it.
        if let UiFlush::Clip(clip) = what {
            let (x, y, w, h) = match clip {
                // Virtual pixels in; `set_scissor_rect` wants physical.
                Some(r) => ui_scale.scissor(&r, vw, vh),
                None => (0, 0, vw, vh),
            };
            pass.set_scissor_rect(x, y, w, h);
            return;
        }
        let id = match what {
            UiFlush::Quad(None) => "__white",
            UiFlush::Quad(Some(id)) | UiFlush::Font(id) => id,
            UiFlush::Clip(_) => unreachable!("handled above"),
        };
        if assets.get_wgpu_texture(id).is_none() {
            if !assets.has_texture_bytes(id) {
                if warned.insert(id.to_string()) {
                    host.warn(&format!("dropping UI draws for missing texture '{id}'"));
                }
                batch.discard_pending();
                return;
            }
            if let Err(e) = assets.ensure_wgpu_texture(id, device, queue) {
                if warned.insert(id.to_string()) {
                    host.warn(&format!("UI upload failed for '{id}': {e:#}"));
                }
                batch.discard_pending();
                return;
            }
        }
        let tex = assets.get_wgpu_texture(id).expect("just ensured");
        batch.flush(device, queue, &mut pass, &ui_vp, tex);
    };
    render_ui_wgpu(world, sprite_batch, Some(frame_fonts), uw, uh, &mut ui_flush);
    render_gui_wgpu(gui_jobs, sprite_batch, Some(frame_fonts), uw, uh, &mut ui_flush);
    wgpu_systems::render_touch_controls_wgpu(touch_draws, sprite_batch, Some(frame_fonts), controls_offset, uh, &mut ui_flush);
    if let Some((state, stats)) = overlay {
        crate::debug_overlay::render_debug_overlay_wgpu(
            state,
            stats,
            world,
            Some(frame_fonts),
            sprite_batch,
            vw as f32,
            vh as f32,
            &mut ui_flush,
        );
    }
}
