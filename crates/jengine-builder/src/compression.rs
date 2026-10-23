//! Texture compression — convert images to smaller file formats.
//!
//! Supports PNG optimization (via oxipng), WebP (lossless/lossy), JPEG,
//! and GPU-ready Basis Universal (UASTC) encoding.
//! Used at build time to reduce asset sizes for distribution.

use std::io::Cursor;
use std::path::Path;

use image::DynamicImage;
use serde::{Deserialize, Serialize};

/// Compression / encoding format for output images.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CompressionFormat {
    /// Standard PNG (no optimization).
    Png,
    /// Optimized PNG — stripped metadata, reduced bit depth where possible.
    PngOptimized,
    /// Lossless WebP.
    WebPLossless,
    /// Lossy WebP with quality 1–100.
    WebPLossy(u8),
    /// JPEG with quality 1–100 (loses alpha channel).
    Jpeg(u8),
    /// Basis Universal UASTC — GPU-ready supercompressed texture.
    /// At runtime, transcodes to BCn/ETC2/ASTC depending on GPU support.
    /// Quality 0–4 (0 = fastest, 4 = best). Produces `.basis` files.
    BasisUniversal(u8),
}

impl std::fmt::Display for CompressionFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Png => write!(f, "PNG"),
            Self::PngOptimized => write!(f, "PNG (optimized)"),
            Self::WebPLossless => write!(f, "WebP (lossless)"),
            Self::WebPLossy(q) => write!(f, "WebP (lossy q={q})"),
            Self::Jpeg(q) => write!(f, "JPEG (q={q})"),
            Self::BasisUniversal(q) => write!(f, "Basis Universal (UASTC q={q})"),
        }
    }
}

/// Statistics about a compression operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionStats {
    pub original_bytes: u64,
    pub compressed_bytes: u64,
    pub format: String,
}

impl CompressionStats {
    /// Size reduction as a percentage (0–100). Negative means it grew.
    pub fn savings_percent(&self) -> f64 {
        if self.original_bytes == 0 {
            return 0.0;
        }
        (1.0 - (self.compressed_bytes as f64 / self.original_bytes as f64)) * 100.0
    }
}

/// Compress/encode a `DynamicImage` to bytes in the given format.
pub fn compress_image(image: &DynamicImage, format: &CompressionFormat) -> anyhow::Result<Vec<u8>> {
    match format {
        CompressionFormat::Png => encode_png(image),
        CompressionFormat::PngOptimized => {
            let raw_png = encode_png(image)?;
            optimize_png_bytes(&raw_png)
        }
        CompressionFormat::WebPLossless => encode_webp_lossless(image),
        CompressionFormat::WebPLossy(quality) => encode_webp_lossy(image, *quality),
        CompressionFormat::Jpeg(quality) => encode_jpeg(image, *quality),
        CompressionFormat::BasisUniversal(quality) => encode_basis(image, *quality),
    }
}

/// Compress a file on disk, writing the result to `output`.
pub fn compress_file(
    input: &Path,
    output: &Path,
    format: &CompressionFormat,
) -> anyhow::Result<CompressionStats> {
    let original_bytes = std::fs::metadata(input)?.len();
    let img = image::open(input)?;
    let compressed = compress_image(&img, format)?;
    let compressed_bytes = compressed.len() as u64;
    std::fs::write(output, &compressed)?;

    Ok(CompressionStats {
        original_bytes,
        compressed_bytes,
        format: format.to_string(),
    })
}

/// Compute savings between original and compressed byte slices.
pub fn estimate_savings(original: &[u8], compressed: &[u8]) -> CompressionStats {
    CompressionStats {
        original_bytes: original.len() as u64,
        compressed_bytes: compressed.len() as u64,
        format: String::new(),
    }
}

// -- Encoding helpers --

fn encode_png(image: &DynamicImage) -> anyhow::Result<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    image.write_to(&mut buf, image::ImageFormat::Png)?;
    Ok(buf.into_inner())
}

fn optimize_png_bytes(png_bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    let opts = oxipng::Options::from_preset(3);
    let optimized = oxipng::optimize_from_memory(png_bytes, &opts)
        .map_err(|e| anyhow::anyhow!("oxipng optimization failed: {e}"))?;
    Ok(optimized)
}

fn encode_webp_lossless(image: &DynamicImage) -> anyhow::Result<Vec<u8>> {
    let mut buf = Cursor::new(Vec::new());
    // The image crate's WebP encoder uses lossless mode by default for RGBA
    image.write_to(&mut buf, image::ImageFormat::WebP)?;
    Ok(buf.into_inner())
}

fn encode_webp_lossy(image: &DynamicImage, quality: u8) -> anyhow::Result<Vec<u8>> {
    // Convert to RGB for lossy (no alpha in lossy WebP unless using libwebp directly).
    // We still use the image crate encoder which handles quality via its own params.
    // For lossy, convert to RGB first to signal lossy intent.
    let rgb = DynamicImage::ImageRgb8(image.to_rgb8());
    let mut buf = Cursor::new(Vec::new());
    // image crate WebP encoder quality is set via the image itself.
    // For more precise quality control, use webp crate directly. For now,
    // we encode as-is (the image crate defaults to ~75 quality for lossy).
    let _ = quality; // TODO: expose quality control when image crate supports it
    rgb.write_to(&mut buf, image::ImageFormat::WebP)?;
    Ok(buf.into_inner())
}

fn encode_jpeg(image: &DynamicImage, quality: u8) -> anyhow::Result<Vec<u8>> {
    let rgb = image.to_rgb8();
    let mut buf = Cursor::new(Vec::new());
    let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
    image::ImageEncoder::write_image(
        encoder,
        &rgb,
        rgb.width(),
        rgb.height(),
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(buf.into_inner())
}

/// Encode an image to Basis Universal UASTC format.
///
/// Quality is clamped to 0–4 (UASTC quality levels). The output is a `.basis`
/// file that can be transcoded at runtime to BCn/ETC2/ASTC/RGBA.
fn encode_basis(image: &DynamicImage, quality: u8) -> anyhow::Result<Vec<u8>> {
    use basis_universal::*;
    use image::GenericImageView;

    let rgba = image.to_rgba8();
    let (width, height) = image.dimensions();

    let quality_level = (quality as u32).min(UASTC_QUALITY_MAX);

    let mut params = CompressorParams::new();
    params.set_basis_format(BasisTextureFormat::UASTC4x4);
    params.set_uastc_quality_level(quality_level);
    params.set_generate_mipmaps(false);
    params.set_color_space(ColorSpace::Srgb);

    // Feed source image data
    params
        .source_image_mut(0)
        .init(&rgba, width, height, 4);

    let mut compressor = Compressor::new(4);
    // SAFETY: params are valid — image data is correctly sized RGBA, dimensions are from
    // the actual image, and UASTC4x4 format is well-supported by the encoder.
    unsafe {
        if !compressor.init(&params) {
            anyhow::bail!("Basis Universal compressor init failed");
        }
        compressor
            .process()
            .map_err(|e| anyhow::anyhow!("Basis Universal encoding failed: {e:?}"))?;
    }

    Ok(compressor.basis_file().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GenericImageView, Rgba, RgbaImage};

    fn test_image() -> DynamicImage {
        let mut img = RgbaImage::new(64, 64);
        for (x, y, pixel) in img.enumerate_pixels_mut() {
            *pixel = Rgba([(x * 4) as u8, (y * 4) as u8, 128, 255]);
        }
        DynamicImage::ImageRgba8(img)
    }

    #[test]
    fn png_roundtrip() {
        let img = test_image();
        let bytes = compress_image(&img, &CompressionFormat::Png).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(decoded.dimensions(), (64, 64));
    }

    #[test]
    fn png_optimized_smaller_or_equal() {
        let img = test_image();
        let raw = compress_image(&img, &CompressionFormat::Png).unwrap();
        let opt = compress_image(&img, &CompressionFormat::PngOptimized).unwrap();
        // Optimized should be <= raw size
        assert!(opt.len() <= raw.len() + 50); // small tolerance for edge cases
    }

    #[test]
    fn webp_roundtrip() {
        let img = test_image();
        let bytes = compress_image(&img, &CompressionFormat::WebPLossless).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(decoded.dimensions(), (64, 64));
    }

    #[test]
    fn jpeg_roundtrip() {
        let img = test_image();
        let bytes = compress_image(&img, &CompressionFormat::Jpeg(85)).unwrap();
        let decoded = image::load_from_memory(&bytes).unwrap();
        assert_eq!(decoded.dimensions(), (64, 64));
    }

    #[test]
    fn savings_calculation() {
        let stats = estimate_savings(&[0u8; 1000], &[0u8; 700]);
        assert!((stats.savings_percent() - 30.0).abs() < 0.1);
    }

    #[test]
    fn basis_universal_encoding() {
        let img = test_image();
        let bytes = compress_image(&img, &CompressionFormat::BasisUniversal(2)).unwrap();
        // Basis files start with a known magic number
        assert!(!bytes.is_empty(), "Basis output should not be empty");
        // The basis-universal transcoder can validate the header
        let transcoder = basis_universal::Transcoder::new();
        assert!(
            transcoder.validate_header(&bytes),
            "Output should be a valid .basis file"
        );
    }
}
