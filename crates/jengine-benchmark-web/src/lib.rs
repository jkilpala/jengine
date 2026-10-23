//! jEngine BunnyMark — WASM/WebGPU benchmark runner.
//!
//! Initializes WebGPU on a `<canvas>`, creates procedural bunny textures, and
//! runs the benchmark in a `requestAnimationFrame` loop. Supports interactive
//! (click) and autoramp modes via URL query parameters.
//!
//! Note: this crate dropped the WebGL2 path in Phase 8.2b — it now requires a
//! browser that supports WebGPU (Chrome/Edge 113+, Safari 18+, Firefox Nightly).

// WASM-only crate.
#![cfg(target_arch = "wasm32")]

use std::cell::RefCell;
use std::rc::Rc;

use glam::{Mat4, UVec2};
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, HtmlElement};

use jengine_benchmark::benchmark::BunnyBenchmark;
use jengine_benchmark::stats::BenchmarkStats;
use jengine_benchmark::texture;
use jengine_core::renderer::backend::wgpu::{WgpuRenderer, WgpuSpriteBatch, WgpuTexture};
use jengine_core::renderer::camera::Camera2D;

const CANVAS_WIDTH: u32 = 1280;
const CANVAS_HEIGHT: u32 = 720;

/// Console.log wrapper.
macro_rules! console_log {
    ($($arg:tt)*) => {
        web_sys::console::log_1(&format!($($arg)*).into())
    };
}

fn window() -> web_sys::Window {
    web_sys::window().expect("no global `window`")
}

fn performance() -> web_sys::Performance {
    window().performance().expect("no `performance`")
}

fn request_animation_frame(f: &Closure<dyn FnMut(f64)>) {
    window()
        .request_animation_frame(f.as_ref().unchecked_ref())
        .expect("requestAnimationFrame failed");
}

fn get_canvas() -> HtmlCanvasElement {
    let document = window().document().expect("no document");
    document
        .get_element_by_id("benchmark-canvas")
        .expect("no #benchmark-canvas element")
        .dyn_into::<HtmlCanvasElement>()
        .expect("element is not a canvas")
}

/// Parse URL query parameters for benchmark configuration.
struct BenchConfig {
    mode: String,
    initial_count: u32,
    add_count: u32,
    target_fps: f32,
    max_frames: u64,
    sort: bool,
    atlas: bool,
}

impl BenchConfig {
    fn from_url() -> Self {
        let mut config = Self {
            mode: "interactive".to_string(),
            initial_count: 100,
            add_count: 100,
            target_fps: 30.0,
            max_frames: 0,
            sort: false,
            atlas: true,
        };

        if let Ok(search) = window().location().search() {
            let params = search.trim_start_matches('?');
            for pair in params.split('&') {
                if let Some((key, val)) = pair.split_once('=') {
                    match key {
                        "mode" => config.mode = val.to_string(),
                        "count" => config.initial_count = val.parse().unwrap_or(100),
                        "add" => config.add_count = val.parse().unwrap_or(100),
                        "target_fps" => config.target_fps = val.parse().unwrap_or(30.0),
                        "frames" => config.max_frames = val.parse().unwrap_or(0),
                        "sort" => config.sort = val == "true" || val == "1",
                        "atlas" => config.atlas = val != "false" && val != "0",
                        _ => {}
                    }
                }
            }
        }

        config
    }
}

/// All benchmark state.
struct WebBenchmark {
    renderer: WgpuRenderer,
    sprite_batch: WgpuSpriteBatch,
    camera: Camera2D,
    bunny_textures: Vec<WgpuTexture>,
    /// Single atlas texture containing all tint variants (optimal — 1-2 draw calls).
    atlas_texture: Option<WgpuTexture>,
    bench: BunnyBenchmark,
    stats: BenchmarkStats,
    config: BenchConfig,
    last_time: f64,
    autoramp_timer: f32,
    click_pending: bool,
    finished: bool,
}

/// Mouse click state shared with the event listener closure.
type ClickFlag = Rc<RefCell<bool>>;

thread_local! {
    static BENCH_STATE: RefCell<Option<Rc<RefCell<WebBenchmark>>>> = const { RefCell::new(None) };
}

/// Get the current benchmark JSON report. Call from JS at any time.
#[wasm_bindgen]
pub fn get_report() -> String {
    BENCH_STATE.with(|cell| {
        cell.borrow()
            .as_ref()
            .map(|s| s.borrow().stats.json_report())
            .unwrap_or_else(|| "{}".to_string())
    })
}

/// WASM entry point. wgpu init is async, so we hand off to `start_async`.
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    console_log!("jEngine BunnyMark (WASM/WebGPU) initializing...");

    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = start_async().await {
            console_log!("BunnyMark init failed: {e:?}");
        }
    });
}

async fn start_async() -> Result<(), JsValue> {
    let config = BenchConfig::from_url();
    console_log!(
        "Config: mode={} count={} add={} target_fps={} frames={} sort={} atlas={}",
        config.mode, config.initial_count, config.add_count,
        config.target_fps, config.max_frames, config.sort, config.atlas,
    );

    let canvas = get_canvas();
    canvas.set_width(CANVAS_WIDTH);
    canvas.set_height(CANVAS_HEIGHT);

    // Create wgpu renderer (async — adapter+device acquisition).
    let renderer = WgpuRenderer::new_from_canvas_async(
        canvas.clone(),
        UVec2::new(CANVAS_WIDTH, CANVAS_HEIGHT),
    )
    .await
    .map_err(|e| JsValue::from_str(&format!("WgpuRenderer init failed: {e}")))?;

    let sprite_batch = WgpuSpriteBatch::new(&renderer.device, renderer.color_format)
        .map_err(|e| JsValue::from_str(&format!("WgpuSpriteBatch init failed: {e}")))?;
    let camera = Camera2D::new(CANVAS_WIDTH as f32, CANVAS_HEIGHT as f32);

    let bunny_textures = texture::create_tinted_bunny_textures_wgpu(&renderer.device, &renderer.queue)
        .map_err(|e| JsValue::from_str(&format!("Failed to create bunny textures: {e}")))?;
    console_log!("Created {} tinted bunny textures", bunny_textures.len());

    let atlas_texture = if config.atlas {
        let atlas = texture::create_bunny_atlas_wgpu(&renderer.device, &renderer.queue)
            .map_err(|e| JsValue::from_str(&format!("Failed to create bunny atlas: {e}")))?;
        console_log!("Atlas mode: ON (single {}x{} texture)", atlas.width, atlas.height);
        Some(atlas)
    } else {
        console_log!("Atlas mode: DISABLED");
        None
    };

    let mut bench = BunnyBenchmark::new(
        CANVAS_WIDTH as f32,
        CANVAS_HEIGHT as f32,
        bunny_textures.len(),
    );
    bench.spawn(config.initial_count as usize);
    console_log!("Spawned {} initial bunnies", config.initial_count);

    // Click listener for interactive mode.
    let click_flag: ClickFlag = Rc::new(RefCell::new(false));
    {
        let flag = click_flag.clone();
        let closure = Closure::<dyn FnMut(_)>::new(move |_event: web_sys::MouseEvent| {
            *flag.borrow_mut() = true;
        });
        canvas
            .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
            .map_err(|e| JsValue::from_str(&format!("Failed to add click listener: {e:?}")))?;
        closure.forget();
    }

    let state = Rc::new(RefCell::new(WebBenchmark {
        renderer,
        sprite_batch,
        camera,
        bunny_textures,
        atlas_texture,
        bench,
        stats: BenchmarkStats::new(),
        config,
        last_time: performance().now(),
        autoramp_timer: 0.0,
        click_pending: false,
        finished: false,
    }));

    BENCH_STATE.with(|cell| {
        *cell.borrow_mut() = Some(state.clone());
    });

    // RAF loop.
    let f: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let g = f.clone();

    let state_loop = state.clone();
    let click_loop = click_flag.clone();

    *g.borrow_mut() = Some(Closure::new(move |timestamp: f64| {
        render_frame(&state_loop, &click_loop, timestamp);

        let s = state_loop.borrow();
        if !s.finished {
            drop(s);
            request_animation_frame(f.borrow().as_ref().unwrap());
        }
    }));

    request_animation_frame(g.borrow().as_ref().unwrap());
    Ok(())
}

/// One frame of the benchmark.
fn render_frame(
    state_cell: &Rc<RefCell<WebBenchmark>>,
    click_cell: &ClickFlag,
    timestamp: f64,
) {
    let mut s = state_cell.borrow_mut();

    if s.finished {
        return;
    }

    if *click_cell.borrow() {
        s.click_pending = true;
        *click_cell.borrow_mut() = false;
    }

    let dt = ((timestamp - s.last_time) / 1000.0) as f32;
    s.last_time = timestamp;
    let dt = dt.min(0.1);

    // Mode logic
    match s.config.mode.as_str() {
        "interactive" => {
            if s.click_pending {
                let add = s.config.add_count;
                s.bench.spawn(add as usize);
                console_log!("Added {} bunnies (total: {})", add, s.bench.count());
            }
        }
        "autoramp" => {
            s.autoramp_timer += dt;
            if s.autoramp_timer >= 1.0 {
                s.autoramp_timer -= 1.0;
                let target = s.config.target_fps;
                if !s.stats.is_below_target(target) {
                    let add = s.config.add_count;
                    s.bench.spawn(add as usize);
                }
            }
            let max_frames = s.config.max_frames;
            if max_frames > 0 && s.stats.total_frames >= max_frames {
                console_log!("Autoramp complete.");
                console_log!("{}", s.stats.json_report());
                update_hud_final(&s.stats);
                s.finished = true;
                return;
            }
        }
        "fixed" => {
            let max_frames = s.config.max_frames;
            if max_frames > 0 && s.stats.total_frames >= max_frames {
                console_log!("Fixed run complete.");
                console_log!("{}", s.stats.json_report());
                update_hud_final(&s.stats);
                s.finished = true;
                return;
            }
        }
        _ => {}
    }
    s.click_pending = false;

    // Update simulation
    s.bench.update(dt);

    // Begin frame
    let mut frame = match s.renderer.begin_frame() {
        Ok(f) => f,
        Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
            let size = s.renderer.size;
            s.renderer.resize(size);
            return;
        }
        Err(e) => {
            console_log!("begin_frame error: {e:?}");
            return;
        }
    };

    // Take wgpu refs out of the borrow so we can pass them into the render
    // pass (which mutably borrows the encoder).
    let WebBenchmark {
        ref mut bench,
        ref mut sprite_batch,
        ref camera,
        ref renderer,
        ref bunny_textures,
        ref atlas_texture,
        ref config,
        ..
    } = *s;

    let vp: Mat4 = camera.view_projection();

    {
        let mut pass = frame.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("bunnymark"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &frame.view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load:  wgpu::LoadOp::Clear(frame.clear_color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        let _ = vp;

        if let Some(atlas) = atlas_texture {
            bench.render_atlas_wgpu(
                sprite_batch,
                camera,
                &renderer.device,
                &renderer.queue,
                &mut pass,
                atlas,
            );
        } else if config.sort {
            bench.render_sorted_wgpu(
                sprite_batch,
                camera,
                &renderer.device,
                &renderer.queue,
                &mut pass,
                bunny_textures,
            );
        } else {
            bench.render_wgpu(
                sprite_batch,
                camera,
                &renderer.device,
                &renderer.queue,
                &mut pass,
                bunny_textures,
            );
        }
    }

    let draw_calls = sprite_batch.frame_draw_calls;
    let sprite_count = sprite_batch.frame_sprite_count;
    let bunny_count = bench.count();

    // Submit + present
    frame.present(&renderer.queue);

    s.stats.record_frame(dt, draw_calls, sprite_count, bunny_count);

    if s.stats.total_frames % 15 == 0 {
        update_hud(&s.stats, &s.config.mode);
    }
}

/// Update the on-page HUD elements.
fn update_hud(stats: &BenchmarkStats, mode: &str) {
    let document = window().document().expect("no document");
    if let Some(el) = document.get_element_by_id("hud-bunnies") {
        let _ = el.dyn_ref::<HtmlElement>().map(|e| {
            e.set_inner_text(&format!("{}", stats.bunny_count));
        });
    }
    if let Some(el) = document.get_element_by_id("hud-fps") {
        let _ = el.dyn_ref::<HtmlElement>().map(|e| {
            e.set_inner_text(&format!("{:.0}", stats.fps()));
        });
    }
    if let Some(el) = document.get_element_by_id("hud-frame") {
        let _ = el.dyn_ref::<HtmlElement>().map(|e| {
            e.set_inner_text(&format!("{:.2}ms", stats.frame_time_ms()));
        });
    }
    if let Some(el) = document.get_element_by_id("hud-draws") {
        let _ = el.dyn_ref::<HtmlElement>().map(|e| {
            e.set_inner_text(&format!("{}", stats.draw_calls));
        });
    }
    if let Some(el) = document.get_element_by_id("hud-mode") {
        let _ = el.dyn_ref::<HtmlElement>().map(|e| {
            e.set_inner_text(mode);
        });
    }
}

/// Show final report in HUD when benchmark completes.
fn update_hud_final(stats: &BenchmarkStats) {
    let document = window().document().expect("no document");
    if let Some(el) = document.get_element_by_id("hud-mode") {
        let _ = el.dyn_ref::<HtmlElement>().map(|e| {
            e.set_inner_text("DONE");
        });
    }
    if let Some(el) = document.get_element_by_id("benchmark-result") {
        let _ = el.dyn_ref::<HtmlElement>().map(|e| {
            e.set_inner_text(&stats.json_report());
        });
    }
}
