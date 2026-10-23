//! jEngine BunnyMark — sprite rendering performance benchmark (wgpu).
//!
//! Measures sprite throughput by spawning thousands of bouncing bunnies.
//! Supports interactive (click-to-add), autoramp (auto-add until FPS drops),
//! and fixed (set count, run N frames) modes.
//!
//! Usage:
//!   cargo run -p jengine-benchmark
//!   cargo run -p jengine-benchmark -- --count 5000 --no-vsync
//!   cargo run -p jengine-benchmark -- --mode autoramp --target-fps 30 --json
//!
//! Renderer: wgpu (Vulkan / Metal / DX12 native, WebGPU/WebGL2 in browser via
//! the sibling `jengine-benchmark-web` crate).

mod benchmark;
mod stats;
mod texture;

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use glam::UVec2;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowAttributes, WindowId};

use jengine_core::renderer::backend::wgpu::{WgpuRenderer, WgpuSpriteBatch, WgpuTexture};
use jengine_core::renderer::camera::Camera2D;

use benchmark::BunnyBenchmark;
use stats::BenchmarkStats;

const WINDOW_WIDTH: u32 = 1280;
const WINDOW_HEIGHT: u32 = 720;

/// jEngine BunnyMark — sprite rendering performance benchmark.
#[derive(Parser, Debug)]
#[command(name = "jengine-benchmark", about = "BunnyMark performance benchmark for jEngine")]
struct Cli {
    /// Initial number of bunnies to spawn.
    #[arg(long, default_value_t = 100)]
    count: u32,

    /// Number of bunnies to add per click (interactive) or per tick (autoramp).
    #[arg(long, default_value_t = 100)]
    add: u32,

    /// Benchmark mode: interactive, autoramp, or fixed.
    #[arg(long, default_value = "interactive")]
    mode: String,

    /// Number of frames to run in fixed/autoramp mode before exiting.
    #[arg(long, default_value_t = 600)]
    frames: u64,

    /// Target FPS threshold for autoramp mode (stops adding when FPS drops below).
    #[arg(long, default_value_t = 30.0)]
    target_fps: f32,

    /// Output JSON report to stdout on exit.
    #[arg(long)]
    json: bool,

    /// Disable VSync for uncapped frame rate (recommended for benchmarks).
    #[arg(long)]
    no_vsync: bool,

    /// Sort bunnies by texture for optimal batching (fewer draw calls, more CPU sort cost).
    #[arg(long)]
    sort: bool,

    /// Disable atlas mode (uses individual textures instead of a single atlas).
    #[arg(long)]
    no_atlas: bool,
}

fn main() -> Result<()> {
    env_logger::init();
    let cli = Cli::parse();

    log::info!("jEngine BunnyMark starting (wgpu)...");
    log::info!(
        "Mode: {} | Initial: {} | Add: {} | VSync: {}",
        cli.mode,
        cli.count,
        cli.add,
        if cli.no_vsync { "OFF" } else { "ON" }
    );

    let event_loop = EventLoop::new()?;
    let mut app = App::new(cli);
    event_loop.run_app(&mut app)?;

    Ok(())
}

/// Application state for the benchmark.
struct App {
    cli: Cli,
    state: Option<AppState>,
}

struct AppState {
    window: Arc<Window>,
    renderer: WgpuRenderer,
    sprite_batch: WgpuSpriteBatch,
    camera: Camera2D,
    bunny_textures: Vec<WgpuTexture>,
    /// Single atlas texture containing all tint variants (optimal — 1-2 draw calls).
    atlas_texture: Option<WgpuTexture>,
    bench: BunnyBenchmark,
    stats: BenchmarkStats,
    last_frame: std::time::Instant,
    /// Accumulator for autoramp — add bunnies every second.
    autoramp_timer: f32,
    /// Mouse left button state tracking.
    mouse_left_pressed: bool,
    mouse_left_just_pressed: bool,
}

impl App {
    fn new(cli: Cli) -> Self {
        Self { cli, state: None }
    }

    fn init(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let window_attrs = WindowAttributes::default()
            .with_title("jEngine BunnyMark")
            .with_inner_size(LogicalSize::new(WINDOW_WIDTH, WINDOW_HEIGHT));
        let window = Arc::new(event_loop.create_window(window_attrs)?);

        let mut renderer = WgpuRenderer::new(window.clone())?;

        // VSync: on by default, off with --no-vsync.
        if self.cli.no_vsync {
            renderer.config.present_mode = wgpu::PresentMode::Immediate;
            if let Some(surface) = &renderer.surface {
                surface.configure(&renderer.device, &renderer.config);
            }
            log::info!("VSync: OFF (uncapped frame rate)");
        } else {
            log::info!("VSync: ON (Fifo)");
        }
        renderer.set_clear_color(0.2, 0.2, 0.25, 1.0);

        let sprite_batch = WgpuSpriteBatch::new(&renderer.device, renderer.color_format)?;
        let (vw, vh) = renderer.viewport_size();
        let camera = Camera2D::new(vw as f32, vh as f32);

        // Create tinted bunny textures for visual variety
        let bunny_textures =
            texture::create_tinted_bunny_textures_wgpu(&renderer.device, &renderer.queue)?;
        log::info!("Created {} tinted bunny textures", bunny_textures.len());

        // Create atlas texture (all tints in one texture) unless disabled
        let atlas_texture = if self.cli.no_atlas {
            log::info!("Atlas mode: DISABLED (using individual textures)");
            None
        } else {
            let atlas = texture::create_bunny_atlas_wgpu(&renderer.device, &renderer.queue)?;
            log::info!("Atlas mode: ON (single {}x{} texture)", atlas.width, atlas.height);
            Some(atlas)
        };

        let mut bench = BunnyBenchmark::new(vw as f32, vh as f32, bunny_textures.len());

        // Spawn initial bunnies
        bench.spawn(self.cli.count as usize);
        log::info!("Spawned {} initial bunnies", self.cli.count);

        self.state = Some(AppState {
            window,
            renderer,
            sprite_batch,
            camera,
            bunny_textures,
            atlas_texture,
            bench,
            stats: BenchmarkStats::new(),
            last_frame: std::time::Instant::now(),
            autoramp_timer: 0.0,
            mouse_left_pressed: false,
            mouse_left_just_pressed: false,
        });

        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_none()
            && let Err(e) = self.init(event_loop)
        {
            log::error!("Failed to initialize: {}", e);
            event_loop.exit();
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(state) = self.state.as_mut() else {
            return;
        };

        match event {
            WindowEvent::CloseRequested => {
                if self.cli.json {
                    println!("{}", state.stats.json_report());
                }
                event_loop.exit();
            }

            WindowEvent::Resized(size) => {
                if size.width > 0 && size.height > 0 {
                    state.renderer.resize(UVec2::new(size.width, size.height));
                    state.camera.resize(size.width as f32, size.height as f32);
                    state.bench.resize(size.width as f32, size.height as f32);
                }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state.is_pressed()
                    && let winit::keyboard::PhysicalKey::Code(
                        winit::keyboard::KeyCode::Escape,
                    ) = event.physical_key
                {
                    if self.cli.json {
                        println!("{}", state.stats.json_report());
                    }
                    event_loop.exit();
                }
            }

            WindowEvent::MouseInput {
                state: btn_state,
                button: winit::event::MouseButton::Left,
                ..
            } => {
                let pressed = btn_state.is_pressed();
                if pressed && !state.mouse_left_pressed {
                    state.mouse_left_just_pressed = true;
                }
                state.mouse_left_pressed = pressed;
            }

            WindowEvent::RedrawRequested => {
                let now = std::time::Instant::now();
                let dt = (now - state.last_frame).as_secs_f32().min(0.1);
                state.last_frame = now;

                // === Mode-specific logic ===
                match self.cli.mode.as_str() {
                    "interactive" => {
                        if state.mouse_left_just_pressed {
                            state.bench.spawn(self.cli.add as usize);
                            log::info!(
                                "Added {} bunnies (total: {})",
                                self.cli.add,
                                state.bench.count()
                            );
                        }
                    }
                    "autoramp" => {
                        state.autoramp_timer += dt;
                        if state.autoramp_timer >= 1.0 {
                            state.autoramp_timer -= 1.0;
                            if !state.stats.is_below_target(self.cli.target_fps) {
                                state.bench.spawn(self.cli.add as usize);
                            }
                        }
                        if state.stats.total_frames >= self.cli.frames {
                            if self.cli.json {
                                println!("{}", state.stats.json_report());
                            }
                            let below = state.stats.is_below_target(self.cli.target_fps);
                            log::info!(
                                "Autoramp complete. Bunnies: {} | FPS: {:.1} | Below target: {}",
                                state.bench.count(),
                                state.stats.fps(),
                                below
                            );
                            event_loop.exit();
                            return;
                        }
                    }
                    "fixed" => {
                        if state.stats.total_frames >= self.cli.frames {
                            if self.cli.json {
                                println!("{}", state.stats.json_report());
                            }
                            log::info!(
                                "Fixed run complete. Bunnies: {} | Avg FPS: {:.1}",
                                state.bench.count(),
                                state.stats.fps()
                            );
                            event_loop.exit();
                            return;
                        }
                    }
                    _ => {
                        log::warn!(
                            "Unknown mode '{}', defaulting to interactive",
                            self.cli.mode
                        );
                    }
                }

                // === Update ===
                state.bench.update(dt);

                // === Render ===
                let mut frame = match state.renderer.begin_frame() {
                    Ok(f) => f,
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        let (w, h) = state.renderer.viewport_size();
                        state.renderer.resize(UVec2::new(w.max(1), h.max(1)));
                        state.window.request_redraw();
                        return;
                    }
                    Err(e) => {
                        log::error!("surface error: {e:?}");
                        state.window.request_redraw();
                        return;
                    }
                };

                {
                    let mut pass = frame.encoder.begin_render_pass(
                        &wgpu::RenderPassDescriptor {
                            label: Some("bunnymark"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: &frame.view,
                                resolve_target: None,
                                ops: wgpu::Operations {
                                    load: wgpu::LoadOp::Clear(frame.clear_color),
                                    store: wgpu::StoreOp::Store,
                                },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: None,
                            occlusion_query_set: None,
                        },
                    );

                    if let Some(atlas) = state.atlas_texture.as_ref() {
                        state.bench.render_atlas_wgpu(
                            &mut state.sprite_batch,
                            &state.camera,
                            &state.renderer.device,
                            &state.renderer.queue,
                            &mut pass,
                            atlas,
                        );
                    } else if self.cli.sort {
                        state.bench.render_sorted_wgpu(
                            &mut state.sprite_batch,
                            &state.camera,
                            &state.renderer.device,
                            &state.renderer.queue,
                            &mut pass,
                            &state.bunny_textures,
                        );
                    } else {
                        state.bench.render_wgpu(
                            &mut state.sprite_batch,
                            &state.camera,
                            &state.renderer.device,
                            &state.renderer.queue,
                            &mut pass,
                            &state.bunny_textures,
                        );
                    }
                }

                // === Stats ===
                let draw_calls = state.sprite_batch.frame_draw_calls;
                let sprite_count = state.sprite_batch.frame_sprite_count;
                state.stats.record_frame(
                    dt,
                    draw_calls,
                    sprite_count,
                    state.bench.count(),
                );
                state.stats.maybe_log(dt);

                // Update window title with live stats
                state.window.set_title(&format!(
                    "jEngine BunnyMark (wgpu) | {} bunnies | {:.0} FPS | {:.1}ms | {} draws",
                    state.bench.count(),
                    state.stats.fps(),
                    state.stats.frame_time_ms(),
                    draw_calls,
                ));

                // === Present ===
                frame.present(&state.renderer.queue);
                state.mouse_left_just_pressed = false;
                state.window.request_redraw();
            }

            _ => {}
        }
    }
}
