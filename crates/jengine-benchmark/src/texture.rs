//! Procedural bunny texture generation.
//!
//! Generates a small recognizable bunny sprite at runtime — no external asset files needed.
//! The bunny is 26×37 pixels (classic BunnyMark dimensions) with a simple silhouette.


/// Bunny sprite dimensions.
pub const BUNNY_WIDTH: u32 = 26;
pub const BUNNY_HEIGHT: u32 = 37;

/// Generate a bunny-shaped RGBA pixel buffer.
///
/// Returns a 26×37 pixel RGBA buffer with a simple bunny silhouette:
/// two ears on top, round head, rectangular body.
fn generate_bunny_pixels(tint: [u8; 3]) -> Vec<u8> {
    let w = BUNNY_WIDTH as usize;
    let h = BUNNY_HEIGHT as usize;
    let mut pixels = vec![0u8; w * h * 4];

    let cx = w as f32 / 2.0;
    let cy = h as f32 / 2.0;

    for y in 0..h {
        for x in 0..w {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let mut alpha: u8 = 0;

            // Left ear: ellipse centered at (8, 6), rx=3, ry=8
            {
                let dx = (fx - 8.0) / 3.0;
                let dy = (fy - 6.0) / 8.0;
                if dx * dx + dy * dy <= 1.0 {
                    alpha = 255;
                }
            }

            // Right ear: ellipse centered at (18, 6), rx=3, ry=8
            {
                let dx = (fx - 18.0) / 3.0;
                let dy = (fy - 6.0) / 8.0;
                if dx * dx + dy * dy <= 1.0 {
                    alpha = 255;
                }
            }

            // Head: circle centered at (cx, 15), r=9
            {
                let dx = fx - cx;
                let dy = fy - 15.0;
                if dx * dx + dy * dy <= 9.0 * 9.0 {
                    alpha = 255;
                }
            }

            // Body: rounded rect from y=20 to y=34, x=5 to x=21
            if (20.0..=34.0).contains(&fy) && (5.0..=21.0).contains(&fx) {
                alpha = 255;
            }

            // Feet: two small ellipses at bottom
            {
                let dx = (fx - 8.0) / 4.0;
                let dy = (fy - 35.0) / 2.5;
                if dx * dx + dy * dy <= 1.0 {
                    alpha = 255;
                }
            }
            {
                let dx = (fx - 18.0) / 4.0;
                let dy = (fy - 35.0) / 2.5;
                if dx * dx + dy * dy <= 1.0 {
                    alpha = 255;
                }
            }

            // Tail: small circle on the right
            {
                let dx = fx - 22.0;
                let dy = fy - 28.0;
                if dx * dx + dy * dy <= 3.0 * 3.0 {
                    alpha = 255;
                }
            }

            // Eyes (darker): two small circles
            let is_eye = {
                let dx1 = fx - 10.0;
                let dy1 = fy - 13.0;
                let dx2 = fx - 16.0;
                let dy2 = fy - 13.0;
                (dx1 * dx1 + dy1 * dy1 <= 2.0 * 2.0)
                    || (dx2 * dx2 + dy2 * dy2 <= 2.0 * 2.0)
            };

            let idx = (y * w + x) * 4;
            if alpha > 0 {
                if is_eye {
                    // Dark eyes
                    pixels[idx] = 30;
                    pixels[idx + 1] = 30;
                    pixels[idx + 2] = 30;
                    pixels[idx + 3] = 255;
                } else {
                    // Slight shading: lighter toward top
                    let shade = 1.0 - (fy / h as f32) * 0.15;
                    pixels[idx] = (tint[0] as f32 * shade).min(255.0) as u8;
                    pixels[idx + 1] = (tint[1] as f32 * shade).min(255.0) as u8;
                    pixels[idx + 2] = (tint[2] as f32 * shade).min(255.0) as u8;
                    pixels[idx + 3] = 255;
                }
            }
            // else: remains transparent (0,0,0,0)
            let _ = cx;
            let _ = cy;
        }
    }

    pixels
}

/// Tint colors used for bunny variants.
pub const TINTS: &[[u8; 3]] = &[
    [255, 255, 255], // White
    [255, 180, 180], // Pink
    [180, 255, 180], // Green
    [180, 180, 255], // Blue
    [255, 255, 180], // Yellow
    [255, 200, 150], // Orange
    [200, 180, 255], // Purple
    [180, 255, 255], // Cyan
];

/// Number of tint variants in the atlas.
pub const TINT_COUNT: usize = 8;

/// Get the UV rect for a specific tint index within the atlas.
///
/// Returns [u_min, v_min, u_max, v_max] for the given tint slot.
pub fn atlas_uv_rect(tint_index: usize) -> [f32; 4] {
    let slot = (tint_index % TINT_COUNT) as f32;
    let u_min = slot / TINT_COUNT as f32;
    let u_max = (slot + 1.0) / TINT_COUNT as f32;
    [u_min, 0.0, u_max, 1.0]
}

// ---------------------------------------------------------------------------
// wgpu texture builders — used by the native binary and the web benchmark.
// ---------------------------------------------------------------------------

#[cfg(any(feature = "native", feature = "web"))]
use jengine_core::renderer::backend::wgpu::WgpuTexture;

/// Create multiple bunny textures (wgpu) with different tint colors.
#[cfg(any(feature = "native", feature = "web"))]
pub fn create_tinted_bunny_textures_wgpu(
    device: &wgpu::Device,
    queue:  &wgpu::Queue,
) -> anyhow::Result<Vec<WgpuTexture>> {
    let mut textures = Vec::with_capacity(TINTS.len());
    for tint in TINTS {
        let pixels = generate_bunny_pixels(*tint);
        textures.push(WgpuTexture::from_rgba(device, queue, BUNNY_WIDTH, BUNNY_HEIGHT, &pixels)?);
    }
    Ok(textures)
}

/// Create a single atlas (wgpu) with all 8 tinted bunnies in a horizontal strip.
#[cfg(any(feature = "native", feature = "web"))]
pub fn create_bunny_atlas_wgpu(
    device: &wgpu::Device,
    queue:  &wgpu::Queue,
) -> anyhow::Result<WgpuTexture> {
    let total_width = BUNNY_WIDTH * TINT_COUNT as u32;
    let mut atlas_pixels = vec![0u8; (total_width * BUNNY_HEIGHT * 4) as usize];

    for (i, tint) in TINTS.iter().enumerate() {
        let src = generate_bunny_pixels(*tint);
        let x_offset = i as u32 * BUNNY_WIDTH;

        for y in 0..BUNNY_HEIGHT {
            let src_row_start = (y * BUNNY_WIDTH * 4) as usize;
            let dst_row_start = (y * total_width * 4 + x_offset * 4) as usize;
            let row_bytes = (BUNNY_WIDTH * 4) as usize;
            atlas_pixels[dst_row_start..dst_row_start + row_bytes]
                .copy_from_slice(&src[src_row_start..src_row_start + row_bytes]);
        }
    }

    WgpuTexture::from_rgba(device, queue, total_width, BUNNY_HEIGHT, &atlas_pixels)
}
