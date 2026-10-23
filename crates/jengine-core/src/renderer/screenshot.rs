//! Screenshots (`Screenshot.take`, Oct 2026; Godot's `get_viewport()
//! .get_texture().get_image()`, Unity's `ScreenCapture`): the finished frame
//! - world, post effects and UI, exactly what the player sees - copied into
//! a texture any sprite or UI image can show (a photo gallery, a save
//! slot's thumbnail), and natively saved as a PNG under the save directory
//! (in the browsers, offered as a download).
//!
//! A script asks ([`request`]) and gets an id; the next frame that is
//! presented is copied out in [`crate::renderer::backend::wgpu::WgpuFrame`]
//! - `begin_frame` makes the readback buffer when something is asked for,
//! `present` copies the swapchain image into it - and when the GPU has
//! finished, [`deliver`] (riding on `ecs::carve::apply_carves`, the step
//! every host makes with its assets) writes the texture and the file and
//! marks the id done ([`is_done`]). So no host was wired: every one presents
//! through `WgpuFrame` and calls `apply_carves`.

use std::cell::RefCell;
use std::collections::HashSet;

use serde_json::Value;

/// One screenshot asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct Request {
    pub id: u64,
    /// The texture to write it into; `None` for none.
    pub texture: Option<String>,
    /// The file (a name under `<save dir>/screenshots/`, `.png` added if
    /// missing); `None` for none.
    pub file: Option<String>,
    /// How much to shrink it by, 0..1 (a thumbnail): 1 is full size.
    pub scale: f32,
}

#[derive(Default)]
struct State {
    next_id: u64,
    queued: Vec<Request>,
    done: HashSet<u64>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Ask for a screenshot of the next frame: `{ texture = "photos/1", file =
/// "shot.png", scale = 0.5 }`. Returns its id, for [`is_done`].
pub fn request(opts: &Value) -> u64 {
    let str_of = |k: &str| opts.get(k).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    let scale = opts.get("scale").and_then(Value::as_f64).unwrap_or(1.0).clamp(0.05, 1.0) as f32;
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.next_id += 1;
        let id = s.next_id;
        s.queued.push(Request { id, texture: str_of("texture"), file: str_of("file"), scale });
        id
    })
}

/// Whether screenshot `id` has been written.
pub fn is_done(id: u64) -> bool {
    STATE.with(|s| s.borrow().done.contains(&id))
}

/// Whether anything is waiting to be captured.
pub fn wanted() -> bool {
    STATE.with(|s| !s.borrow().queued.is_empty())
}

/// What is waiting, taken.
pub fn take_queued() -> Vec<Request> {
    STATE.with(|s| std::mem::take(&mut s.borrow_mut().queued))
}

fn mark_done(id: u64) {
    STATE.with(|s| {
        s.borrow_mut().done.insert(id);
    });
}

/// `w` x `h` RGBA bytes shrunk by `scale` (box filter).
pub fn shrink(rgba: &[u8], w: u32, h: u32, scale: f32) -> (Vec<u8>, u32, u32) {
    if scale >= 0.999 {
        return (rgba.to_vec(), w, h);
    }
    let (nw, nh) = (((w as f32 * scale).round() as u32).max(1), ((h as f32 * scale).round() as u32).max(1));
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        let (y0, y1) = (y * h / nh, ((y + 1) * h / nh).max(y * h / nh + 1));
        for x in 0..nw {
            let (x0, x1) = (x * w / nw, ((x + 1) * w / nw).max(x * w / nw + 1));
            let mut acc = [0u32; 4];
            for sy in y0..y1.min(h) {
                for sx in x0..x1.min(w) {
                    let i = ((sy * w + sx) * 4) as usize;
                    for c in 0..4 {
                        acc[c] += rgba[i + c] as u32;
                    }
                }
            }
            let n = ((y1.min(h) - y0) * (x1.min(w) - x0)).max(1);
            let o = ((y * nw + x) * 4) as usize;
            for c in 0..4 {
                out[o + c] = (acc[c] / n) as u8;
            }
        }
    }
    (out, nw, nh)
}

/// Copied rows (each `padded` bytes) of a `format` texture as tight RGBA,
/// opaque.
pub fn unpad_rgba(data: &[u8], w: u32, h: u32, padded: u32, bgra: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        let row = &data[(y * padded) as usize..(y * padded + w * 4) as usize];
        for px in row.chunks_exact(4) {
            if bgra {
                out.extend_from_slice(&[px[2], px[1], px[0], 255]);
            } else {
                out.extend_from_slice(&[px[0], px[1], px[2], 255]);
            }
        }
    }
    out
}

/// Where a screenshot file goes: `<JENGINE_SAVE_DIR or saves>/screenshots/`.
pub fn file_path(name: &str) -> std::path::PathBuf {
    let base = std::env::var("JENGINE_SAVE_DIR").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "saves".into());
    let mut name = name.replace(['\\', ':'], "_").replace("..", "_");
    if !name.to_ascii_lowercase().ends_with(".png") {
        name.push_str(".png");
    }
    std::path::PathBuf::from(base).join("screenshots").join(name)
}

/// Write finished pixels where each request asked: into its texture, and to
/// its file.
pub fn write(requests: &[Request], rgba: &[u8], w: u32, h: u32, assets: &mut crate::asset::AssetManager) {
    for r in requests {
        let (px, pw, ph) = shrink(rgba, w, h, r.scale);
        if let Some(id) = &r.texture {
            assets.insert_texture_bytes(id.clone(), crate::asset::RawTextureData { width: pw, height: ph, rgba: px.clone() });
            assets.mark_texture_dirty(id, 0, 0, pw, ph);
        }
        if let Some(name) = &r.file {
            save_png(name, &px, pw, ph);
        }
        mark_done(r.id);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn save_png(name: &str, rgba: &[u8], w: u32, h: u32) {
    let path = file_path(name);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    match image::save_buffer(&path, rgba, w, h, image::ExtendedColorType::Rgba8) {
        Ok(()) => log::info!("[screenshot] saved {}", path.display()),
        Err(e) => log::warn!("[screenshot] could not save {}: {e}", path.display()),
    }
}

/// In a browser a file is offered as a download.
#[cfg(target_arch = "wasm32")]
fn save_png(name: &str, rgba: &[u8], w: u32, h: u32) {
    use wasm_bindgen::JsCast;
    let mut png = Vec::new();
    if let Err(e) = image::codecs::png::PngEncoder::new(&mut png).write_image(rgba, w, h, image::ExtendedColorType::Rgba8) {
        log::warn!("[screenshot] could not encode {name}: {e}");
        return;
    }
    let file = file_path(name).file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_else(|| "screenshot.png".into());
    let go = || -> Option<()> {
        let bytes = js_sys::Uint8Array::from(png.as_slice());
        let parts = js_sys::Array::of1(&bytes);
        let opts = web_sys::BlobPropertyBag::new();
        opts.set_type("image/png");
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &opts).ok()?;
        let url = web_sys::Url::create_object_url_with_blob(&blob).ok()?;
        let doc = web_sys::window()?.document()?;
        let a: web_sys::HtmlAnchorElement = doc.create_element("a").ok()?.dyn_into().ok()?;
        a.set_href(&url);
        a.set_download(&file);
        a.click();
        let _ = web_sys::Url::revoke_object_url(&url);
        Some(())
    };
    if go().is_none() {
        log::warn!("[screenshot] could not offer {file} as a download");
    }
}

#[cfg(target_arch = "wasm32")]
use image::ImageEncoder;

/// The GPU side: a frame's copy into a readback buffer, and its mapping.
#[cfg(feature = "gpu-wgpu")]
pub mod gpu {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU8, Ordering};

    /// A copy set up for one frame.
    pub struct Capture {
        buffer: wgpu::Buffer,
        width: u32,
        height: u32,
        padded: u32,
        bgra: bool,
        requests: Vec<Request>,
    }

    /// A copy submitted and being mapped: 0 waiting, 1 mapped, 2 failed.
    struct InFlight {
        buffer: wgpu::Buffer,
        width: u32,
        height: u32,
        padded: u32,
        bgra: bool,
        requests: Vec<Request>,
        state: Arc<AtomicU8>,
    }

    thread_local! {
        static IN_FLIGHT: RefCell<Option<InFlight>> = const { RefCell::new(None) };
    }

    /// Natively a map completes when the device is polled: once a frame,
    /// while a copy is in flight. (A browser completes it by itself.)
    pub fn poll(device: &wgpu::Device) {
        if IN_FLIGHT.with(|f| f.borrow().is_some()) {
            device.poll(wgpu::Maintain::Poll);
        }
    }

    /// At the start of a frame: a readback buffer for it, if a screenshot
    /// is wanted and none is still in flight.
    pub fn begin(device: &wgpu::Device, width: u32, height: u32, format: wgpu::TextureFormat) -> Option<Capture> {
        if !wanted() || IN_FLIGHT.with(|f| f.borrow().is_some()) || width == 0 || height == 0 {
            return None;
        }
        let bgra = matches!(format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
        if !bgra && !matches!(format, wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb) {
            log::warn!("[screenshot] the surface is {format:?}, which cannot be read back - nothing taken");
            take_queued();
            return None;
        }
        let padded = (width * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("screenshot"),
            size: (padded * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        Some(Capture { buffer, width, height, padded, bgra, requests: take_queued() })
    }

    impl Capture {
        /// Record the copy of the finished frame, last in its encoder.
        pub fn encode(&self, encoder: &mut wgpu::CommandEncoder, texture: &wgpu::Texture) {
            encoder.copy_texture_to_buffer(
                texture.as_image_copy(),
                wgpu::ImageCopyBuffer {
                    buffer: &self.buffer,
                    layout: wgpu::ImageDataLayout {
                        offset: 0,
                        bytes_per_row: Some(self.padded),
                        rows_per_image: Some(self.height),
                    },
                },
                wgpu::Extent3d { width: self.width, height: self.height, depth_or_array_layers: 1 },
            );
        }

        /// After the submit: start mapping it; [`deliver`] finishes.
        pub fn submitted(self) {
            let state = Arc::new(AtomicU8::new(0));
            let s = Arc::clone(&state);
            self.buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                s.store(if r.is_ok() { 1 } else { 2 }, Ordering::Release);
            });
            IN_FLIGHT.with(|f| {
                *f.borrow_mut() = Some(InFlight {
                    buffer: self.buffer,
                    width: self.width,
                    height: self.height,
                    padded: self.padded,
                    bgra: self.bgra,
                    requests: self.requests,
                    state,
                })
            });
        }
    }

    /// Once a frame: a copy the GPU has finished is written where it was
    /// asked for. (Natively the map completes on a later submit's poll.)
    pub fn deliver(assets: &mut crate::asset::AssetManager) {
        let ready = IN_FLIGHT.with(|f| f.borrow().as_ref().map(|c| c.state.load(Ordering::Acquire)));
        match ready {
            Some(1) => {}
            Some(2) => {
                log::warn!("[screenshot] the copy could not be read back");
                IN_FLIGHT.with(|f| f.borrow_mut().take());
                return;
            }
            _ => return,
        }
        let Some(c) = IN_FLIGHT.with(|f| f.borrow_mut().take()) else { return };
        let rgba = {
            let view = c.buffer.slice(..).get_mapped_range();
            unpad_rgba(&view, c.width, c.height, c.padded, c.bgra)
        };
        c.buffer.unmap();
        write(&c.requests, &rgba, c.width, c.height, assets);
    }
}
