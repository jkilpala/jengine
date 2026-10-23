//! BunnyMark benchmark logic — bunny simulation and rendering.
//!
//! Bunnies bounce around the screen with gravity. Two rendering modes:
//! - Direct: plain Vec + SpriteBatch (raw GPU throughput test)
//! - ECS: hecs entities with Transform + SpriteComponent (engine pipeline test)

use glam::{Vec2, Vec4};

use jengine_core::renderer::camera::Camera2D;

/// Gravity applied to bunnies (pixels/sec²), pulling downward.
const GRAVITY: f32 = 500.0;

/// Platform-specific RNG seed.
#[cfg(not(target_arch = "wasm32"))]
fn platform_seed() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(42)
}

/// Platform-specific RNG seed (WASM — use js_sys::Math::random).
#[cfg(target_arch = "wasm32")]
fn platform_seed() -> u32 {
    (js_sys::Math::random() * u32::MAX as f64) as u32
}

/// Simple pseudo-random number generator (xorshift32).
/// We avoid pulling in `rand` for this — benchmark should be self-contained.
pub struct Rng {
    state: u32,
}

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self { state: if seed == 0 { 1 } else { seed } }
    }

    /// Returns a u32 in [0, u32::MAX].
    fn next_u32(&mut self) -> u32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        self.state
    }

    /// Returns a float in [0.0, 1.0).
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u32() as f32) / (u32::MAX as f32)
    }

    /// Returns a float in [min, max).
    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        min + self.next_f32() * (max - min)
    }

    /// Returns a usize in [0, max).
    pub fn next_usize(&mut self, max: usize) -> usize {
        (self.next_u32() as usize) % max
    }
}

/// A single bunny in the direct-mode simulation.
struct Bunny {
    position: Vec2,
    velocity: Vec2,
    /// Index into the tinted texture array for visual variety.
    texture_index: usize,
}

/// BunnyMark benchmark state.
pub struct BunnyBenchmark {
    bunnies: Vec<Bunny>,
    rng: Rng,
    /// Screen half-dimensions (coordinate system: origin at center, ±half).
    half_width: f32,
    half_height: f32,
    /// Number of tinted textures available.
    texture_count: usize,
}

impl BunnyBenchmark {
    /// Create a new benchmark with the given screen dimensions.
    pub fn new(screen_width: f32, screen_height: f32, texture_count: usize) -> Self {
        // Use current time as seed for variety
        let seed = platform_seed();

        Self {
            bunnies: Vec::new(),
            rng: Rng::new(seed),
            half_width: screen_width / 2.0,
            half_height: screen_height / 2.0,
            texture_count: texture_count.max(1),
        }
    }

    /// Spawn `count` bunnies at the top-center with random velocities.
    pub fn spawn(&mut self, count: usize) {
        self.bunnies.reserve(count);
        for _ in 0..count {
            self.bunnies.push(Bunny {
                position: Vec2::new(0.0, self.half_height - 20.0), // Top center
                velocity: Vec2::new(
                    self.rng.range(-250.0, 250.0),
                    self.rng.range(-50.0, 150.0),
                ),
                texture_index: self.rng.next_usize(self.texture_count),
            });
        }
    }

    /// Update all bunnies: apply gravity, move, bounce off walls.
    pub fn update(&mut self, dt: f32) {
        let hw = self.half_width;
        let hh = self.half_height;

        for bunny in &mut self.bunnies {
            // Apply gravity (downward in screen space with +Y up)
            bunny.velocity.y -= GRAVITY * dt;

            // Move
            bunny.position += bunny.velocity * dt;

            // Bounce off horizontal walls
            if bunny.position.x < -hw {
                bunny.position.x = -hw;
                bunny.velocity.x = bunny.velocity.x.abs() * self.rng.range(0.8, 1.0);
            } else if bunny.position.x > hw {
                bunny.position.x = hw;
                bunny.velocity.x = -(bunny.velocity.x.abs() * self.rng.range(0.8, 1.0));
            }

            // Bounce off floor
            if bunny.position.y < -hh {
                bunny.position.y = -hh;
                bunny.velocity.y = bunny.velocity.y.abs() * self.rng.range(0.8, 0.95);
                // Add random horizontal perturbation on bounce
                bunny.velocity.x += self.rng.range(-30.0, 30.0);
            }

            // Bounce off ceiling
            if bunny.position.y > hh {
                bunny.position.y = hh;
                bunny.velocity.y = -(bunny.velocity.y.abs() * self.rng.range(0.8, 1.0));
            }
        }
    }

    /// Number of bunnies currently alive.
    pub fn count(&self) -> u32 {
        self.bunnies.len() as u32
    }

    /// Update screen dimensions (e.g. on window resize).
    pub fn resize(&mut self, width: f32, height: f32) {
        self.half_width = width / 2.0;
        self.half_height = height / 2.0;
    }
}

// ---------------------------------------------------------------------------
// wgpu render variants — used by the native binary and the web benchmark.
// Three strategies: per-texture flush, pre-sorted by texture, single atlas.
// ---------------------------------------------------------------------------

#[cfg(any(feature = "native", feature = "web"))]
use jengine_core::renderer::backend::wgpu::{WgpuSpriteBatch, WgpuTexture};

#[cfg(any(feature = "native", feature = "web"))]
impl BunnyBenchmark {
    /// Render all bunnies (wgpu) flushing on texture changes — mirrors `render`.
    #[allow(clippy::too_many_arguments)]
    pub fn render_wgpu(
        &self,
        batch:    &mut WgpuSpriteBatch,
        camera:   &Camera2D,
        device:   &wgpu::Device,
        queue:    &wgpu::Queue,
        pass:     &mut wgpu::RenderPass<'_>,
        textures: &[WgpuTexture],
    ) {
        let vp = camera.view_projection();
        let bunny_size = Vec2::new(
            crate::texture::BUNNY_WIDTH as f32,
            crate::texture::BUNNY_HEIGHT as f32,
        );
        let white = Vec4::ONE;
        let full_uv = [0.0f32, 0.0, 1.0, 1.0];
        let center_anchor = [0.5f32, 0.5];

        batch.begin_frame();
        let mut current_tex_idx = usize::MAX;

        for bunny in &self.bunnies {
            if bunny.texture_index != current_tex_idx {
                if current_tex_idx != usize::MAX {
                    let tex = &textures[current_tex_idx % textures.len()];
                    batch.flush(device, queue, pass, &vp, tex);
                }
                current_tex_idx = bunny.texture_index;
            }
            if batch.is_full() {
                let tex = &textures[current_tex_idx % textures.len()];
                batch.flush(device, queue, pass, &vp, tex);
            }
            batch.draw_sprite_anchored(
                bunny.position, bunny_size, 0.0, white, full_uv, center_anchor,
            );
        }
        if current_tex_idx != usize::MAX {
            let tex = &textures[current_tex_idx % textures.len()];
            batch.flush(device, queue, pass, &vp, tex);
        }
    }

    /// Render all bunnies (wgpu) sorted by texture — mirrors `render_sorted`.
    #[allow(clippy::too_many_arguments)]
    pub fn render_sorted_wgpu(
        &mut self,
        batch:    &mut WgpuSpriteBatch,
        camera:   &Camera2D,
        device:   &wgpu::Device,
        queue:    &wgpu::Queue,
        pass:     &mut wgpu::RenderPass<'_>,
        textures: &[WgpuTexture],
    ) {
        let mut indices: Vec<usize> = (0..self.bunnies.len()).collect();
        indices.sort_unstable_by_key(|&i| self.bunnies[i].texture_index);

        let vp = camera.view_projection();
        let bunny_size = Vec2::new(
            crate::texture::BUNNY_WIDTH as f32,
            crate::texture::BUNNY_HEIGHT as f32,
        );
        let white = Vec4::ONE;
        let full_uv = [0.0f32, 0.0, 1.0, 1.0];
        let center_anchor = [0.5f32, 0.5];

        batch.begin_frame();
        let mut current_tex_idx = usize::MAX;

        for &idx in &indices {
            let bunny = &self.bunnies[idx];
            if bunny.texture_index != current_tex_idx {
                if current_tex_idx != usize::MAX {
                    let tex = &textures[current_tex_idx % textures.len()];
                    batch.flush(device, queue, pass, &vp, tex);
                }
                current_tex_idx = bunny.texture_index;
            }
            if batch.is_full() {
                let tex = &textures[current_tex_idx % textures.len()];
                batch.flush(device, queue, pass, &vp, tex);
            }
            batch.draw_sprite_anchored(
                bunny.position, bunny_size, 0.0, white, full_uv, center_anchor,
            );
        }
        if current_tex_idx != usize::MAX {
            let tex = &textures[current_tex_idx % textures.len()];
            batch.flush(device, queue, pass, &vp, tex);
        }
    }

    /// Render all bunnies (wgpu) using a single atlas — mirrors `render_atlas`.
    #[allow(clippy::too_many_arguments)]
    pub fn render_atlas_wgpu(
        &self,
        batch:  &mut WgpuSpriteBatch,
        camera: &Camera2D,
        device: &wgpu::Device,
        queue:  &wgpu::Queue,
        pass:   &mut wgpu::RenderPass<'_>,
        atlas:  &WgpuTexture,
    ) {
        let vp = camera.view_projection();
        let bunny_size = Vec2::new(
            crate::texture::BUNNY_WIDTH as f32,
            crate::texture::BUNNY_HEIGHT as f32,
        );
        let white = Vec4::ONE;
        let center_anchor = [0.5f32, 0.5];

        batch.begin_frame();

        for bunny in &self.bunnies {
            if batch.is_full() {
                batch.flush(device, queue, pass, &vp, atlas);
            }
            let uv = crate::texture::atlas_uv_rect(bunny.texture_index);
            batch.draw_sprite_anchored(
                bunny.position, bunny_size, 0.0, white, uv, center_anchor,
            );
        }
        batch.flush(device, queue, pass, &vp, atlas);
    }
}
