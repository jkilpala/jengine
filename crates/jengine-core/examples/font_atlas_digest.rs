//! Print a digest of the built-in font atlas.
//!
//! `built_in_font_atlas()` bakes the embedded TTF into a glyph atlas at
//! startup on every runtime, so any change to that font file has to be
//! proven glyph-for-glyph identical. Run this before and after touching
//! `assets/fonts/`, and diff the output:
//!
//! ```text
//! cargo run -p jengine-core --features gpu-wgpu --example font_atlas_digest
//! ```

fn main() {
    let (rgba, w, h, font) = jengine_core::renderer::text::built_in_font_atlas();

    // FNV-1a over the pixel data — enough to catch a single changed pixel
    // without pulling in a hashing dependency.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in &rgba {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }

    println!("atlas       {w}x{h}  ({} bytes)", rgba.len());
    println!("pixel hash  {hash:#018x}");
    println!("line height {:.4}", font.line_height);
    println!("glyphs      {}", font.glyphs.len());
    for g in &font.glyphs {
        println!(
            "  {:?} at {},{} {}x{} adv {:.4} off {},{}",
            g.ch, g.x, g.y, g.width, g.height, g.advance, g.x_offset, g.y_offset
        );
    }
}
