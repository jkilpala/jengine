//! Asset manager — caches raw texture bytes, atlas metadata, audio clips,
//! and GUI documents. GPU textures are uploaded lazily on the wgpu side
//! via [`AssetManager::ensure_wgpu_texture`].
//!
//! Phase 8 dropped the legacy gl `Texture` cache; everything sprite-side
//! now flows through `texture_bytes` (raw RGBA8) and the wgpu texture map.

pub mod atlas_def;
pub mod sort_layers;
pub mod asset_id;
pub use asset_id::{AssetId, MaterialId, TextureId};
pub mod watcher;
/// Parts of a game downloaded when needed (`settings.asset_packs`).
#[cfg(feature = "packs")]
pub mod packs;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Find asset path `rel` on disk: under each of `roots` in turn (the first
/// existing file wins), then in the mounted asset packs
/// ([`packs::locate`]), then under the game's own roots
/// ([`packs::set_base_roots`] - a scene loaded out of a pack has candidate
/// roots inside the pack, and still names the game's own textures).
///
/// The one rule every native loader resolves a file by, so a file a pack
/// brought loads wherever a file of the build would.
pub fn find_in_roots(rel: &str, roots: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    for root in roots {
        let candidate = if root.as_os_str().is_empty() { std::path::PathBuf::from(rel) } else { root.join(rel) };
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    #[cfg(feature = "packs")]
    if let Some(p) = packs::locate(rel)
        && p.is_file()
    {
        return Some(p);
    }
    #[cfg(feature = "packs")]
    return packs::base_roots()
        .into_iter()
        .filter(|b| !roots.contains(b))
        .map(|b| b.join(rel))
        .find(|c| c.is_file());
    #[cfg(not(feature = "packs"))]
    return None;
}

use crate::animation::rig::SkeletonRig;
use crate::animation::skeletal::SkeletalClip;
use crate::animation::{AnimationClip, AnimationFrame};
use crate::asset::sort_layers::SortLayers;
use crate::renderer::atlas::{AtlasData, AtlasRegion};
use crate::renderer::material::MaterialData;
use crate::renderer::text::FontData;
use crate::ui::gui_document::GuiDocument;

/// Texture id of the built-in anti-aliased disc, registered by
/// [`AssetManager::new`].
///
/// The sprite pipeline draws textured quads and nothing else, so a round
/// shape has to come from the texture. Renderers bind this for GUI `Circle`
/// nodes; anything else wanting a dot can use it too.
pub const CIRCLE_TEXTURE_ID: &str = "__circle";

/// Texture id of the built-in 1x1 white pixel every host registers.
///
/// The sprite pipeline draws textured quads and nothing else, so a plain
/// coloured rectangle is this texture tinted by the sprite's colour. Named
/// here because a `SpriteComponent` with an **empty** `texture_id` resolves to
/// it: an authored sprite with no texture obviously means "a coloured quad",
/// and the alternative was that it silently drew nothing at all — the renderer
/// discards a batch whose texture id is not registered, and `""` never is.
pub const WHITE_TEXTURE_ID: &str = "__white";

/// The texture a sprite should actually be drawn with.
///
/// Empty means "tint the white pixel"; anything else is used as given.
///
/// A function rather than two inline `if`s because the renderer collects
/// drawables in two places (the plain path and the shadow-interleaved one) and
/// a fix applied to one of them is a bug that only shows up in scenes with a
/// `ShadowCaster`.
#[inline]
pub fn resolve_texture_id(texture_id: &str) -> &str {
    if texture_id.is_empty() {
        WHITE_TEXTURE_ID
    } else {
        texture_id
    }
}

/// Size of the built-in disc. 64px is enough that the anti-aliased edge still
/// reads as smooth when a Circle node is scaled up to a few hundred pixels,
/// and it costs 16 KB.
const CIRCLE_TEXTURE_SIZE: u32 = 64;

/// Build the built-in disc: white RGB with coverage in alpha, so a renderer
/// tints it with the node's fill colour the same way it tints `__white`.
///
/// Coverage is computed from the pixel-centre distance to the middle, with a
/// one-pixel linear ramp at the rim. That is cheap and, unlike a hard cutoff,
/// does not leave a staircase edge on a scaled-up circle.
fn built_in_circle_texture() -> RawTextureData {
    let size = CIRCLE_TEXTURE_SIZE;
    let radius = size as f32 * 0.5;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let dx = (x as f32 + 0.5) - radius;
            let dy = (y as f32 + 0.5) - radius;
            let dist = (dx * dx + dy * dy).sqrt();
            // 1.0 inside, ramping to 0.0 across the outermost pixel.
            let coverage = (radius - dist).clamp(0.0, 1.0);
            rgba.extend_from_slice(&[255, 255, 255, (coverage * 255.0) as u8]);
        }
    }
    RawTextureData { width: size, height: size, rgba }
}


/// Decoded RGBA8 pixel buffer + dimensions, kept in CPU memory for the
/// lifetime of the asset cache. The wgpu backend uploads from these
/// bytes on first use via [`AssetManager::ensure_wgpu_texture`].
#[derive(Debug, Clone)]
pub struct RawTextureData {
    pub width: u32,
    pub height: u32,
    /// Tightly packed RGBA8 (`width * height * 4` bytes).
    pub rgba: Vec<u8>,
}

impl RawTextureData {
    /// Decode any PNG/JPEG/etc. byte slice into RGBA8.
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        let img = image::load_from_memory(bytes)?.to_rgba8();
        let (width, height) = img.dimensions();
        Ok(Self { width, height, rgba: img.into_raw() })
    }

    /// Decode an image from disk into RGBA8 (native only).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let img = image::open(path)?.to_rgba8();
        let (width, height) = img.dimensions();
        Ok(Self { width, height, rgba: img.into_raw() })
    }
}

/// Asset manager: raw texture bytes, atlas metadata, audio, GUI documents,
/// and a lazy wgpu GPU texture cache.
pub struct AssetManager {
    /// Atlas metadata keyed by atlas id (e.g. "_atlases/characters").
    atlas_data: HashMap<String, AtlasData>,
    /// Maps original sprite paths → (atlas_id, region_name).
    atlas_lookup: HashMap<String, (String, String)>,
    /// Raw audio data keyed by clip ID (e.g. "sounds/jump.ogg").
    audio_clips: HashMap<String, Vec<u8>>,
    /// Bitmap fonts keyed by font id - the `.font` asset path a
    /// `TextComponent.font_id` names (e.g. "fonts/bebasneue-regular.font").
    /// The matching atlas image lives in `texture_bytes` under the id
    /// [`font_texture_id`] derives, so a font is two entries that are
    /// inserted and looked up together.
    ///
    /// Behind an `Arc` so the render path can lift the frame's fonts out
    /// before the draw closures take `&mut self` - see
    /// [`crate::renderer::font_asset::FrameFonts`]. Cloning the map's values
    /// is then a refcount bump rather than a copy of every glyph.
    fonts: HashMap<String, Arc<FontData>>,
    /// GUI document assets keyed by ID (e.g. "ui/hud.gui").
    gui_documents: HashMap<String, GuiDocument>,
    /// Material assets keyed by ID (typically the `.material` file's relative path
    /// or a logical name registered by user code). Loaded lazily via
    /// [`Self::load_material`].
    materials: HashMap<String, MaterialData>,
    /// Slice 3.5.C — skeletal animation clips keyed by asset id.
    /// Loaded from `.skelanim` JSON files or registered programmatically
    /// by importers (slice 3.5.D Spine importer).
    skeletal_clips: HashMap<String, SkeletalClip>,
    /// Slice 3.5.D — authored skeleton rigs keyed by asset id.
    /// Loaded from `.skel` JSON, imported from Spine via
    /// [`Self::load_spine_skeleton`], or inserted programmatically.
    skeleton_rigs: HashMap<String, SkeletonRig>,
    /// `.sortlayers` assets keyed by ID. Scenes pick which set is
    /// active via `Self::set_active_sort_layers`.
    sort_layer_sets: HashMap<String, SortLayers>,
    /// ID of the currently active sort-layer set, or `None` to fall
    /// back to raw `z_order` values.
    active_sort_layers: Option<String>,
    /// Empty fallback returned from `Self::active_sort_layers` when no
    /// set is active.
    empty_sort_layers: SortLayers,
    /// Decoded RGBA8 byte cache, keyed by texture id.
    texture_bytes: HashMap<String, RawTextureData>,
    /// Lazily-uploaded wgpu textures, materialized on first call to
    /// [`Self::ensure_wgpu_texture`]. Cleared by [`Self::destroy_wgpu_textures`].
    #[cfg(feature = "gpu-wgpu")]
    wgpu_textures: HashMap<String, crate::renderer::backend::wgpu::WgpuTexture>,
    /// Texture ids that are signed distance fields (`mark_sdf`).
    sdf_textures: std::collections::HashSet<String>,
    /// How each texture is sampled when it is uploaded.
    texture_filters: crate::renderer::texture_filter::TextureFilterRules,
    /// Rectangles `(x, y, w, h)` of textures whose bytes changed after they
    /// were uploaded ([`Self::mark_texture_dirty`]), written to the GPU by the
    /// next [`Self::ensure_wgpu_texture`]. Several a texture, so two edits
    /// far apart are not one rectangle spanning the texture.
    dirty_rects: HashMap<String, Vec<(u32, u32, u32, u32)>>,
    /// How many times each texture's pixels have been replaced or changed
    /// ([`Self::texture_revision`]).
    texture_revisions: HashMap<String, u64>,
    /// The mip levels below the base of each mipmapped texture that has been
    /// changed at runtime, kept so the next change patches only the texels
    /// it reaches (`texture_filter::update_mip_chain`). Made on a texture's
    /// first change, dropped when its bytes or its upload are replaced.
    #[cfg(feature = "gpu-wgpu")]
    mip_cache: HashMap<String, Vec<(u32, u32, Vec<u8>)>>,
    /// Draws a linear texture's mip levels on the GPU when it is uploaded
    /// (`renderer::backend::wgpu::mipmap`). Made on the first upload, for the
    /// device the textures here live on, and dropped with them.
    #[cfg(feature = "gpu-wgpu")]
    mip_generator: Option<crate::renderer::backend::wgpu::MipGenerator>,
}

/// Upload `raw` filtered by `filter`: a linear texture's mip levels drawn on
/// the GPU (the generator made on first need), a nearest one a single level.
#[cfg(feature = "gpu-wgpu")]
fn upload_filtered(
    generator: &mut Option<crate::renderer::backend::wgpu::MipGenerator>,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    raw: &RawTextureData,
    filter: crate::renderer::texture_filter::TextureFilter,
    srgb: bool,
) -> anyhow::Result<crate::renderer::backend::wgpu::WgpuTexture> {
    use crate::renderer::backend::wgpu::{MipGenerator, WgpuTexture};
    use crate::renderer::texture_filter::TextureFilter;
    match filter {
        TextureFilter::Nearest => WgpuTexture::from_rgba_filtered(device, queue, raw.width, raw.height, &raw.rgba, filter, srgb),
        TextureFilter::Linear => {
            let mips = generator.get_or_insert_with(|| MipGenerator::new(device));
            WgpuTexture::from_rgba_filtered_gpu_mips(device, queue, mips, raw.width, raw.height, &raw.rgba, filter, srgb)
        }
    }
}

impl Default for AssetManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AssetManager {
    pub fn new() -> Self {
        let mut this = Self {
            atlas_data: HashMap::new(),
            atlas_lookup: HashMap::new(),
            audio_clips: HashMap::new(),
            fonts: HashMap::new(),
            gui_documents: HashMap::new(),
            materials: HashMap::new(),
            skeletal_clips: HashMap::new(),
            skeleton_rigs: HashMap::new(),
            sort_layer_sets: HashMap::new(),
            active_sort_layers: None,
            empty_sort_layers: SortLayers::default(),
            texture_bytes: HashMap::new(),
            #[cfg(feature = "gpu-wgpu")]
            wgpu_textures: HashMap::new(),
            sdf_textures: Default::default(),
            texture_filters: Default::default(),
            dirty_rects: HashMap::new(),
            texture_revisions: HashMap::new(),
            #[cfg(feature = "gpu-wgpu")]
            mip_cache: HashMap::new(),
            #[cfg(feature = "gpu-wgpu")]
            mip_generator: None,
        };
        // Built in rather than inserted by each host: three separate
        // registrations is how `__white` ended up duplicated in the viewport,
        // the runtime and the web build.
        this.texture_bytes
            .insert(CIRCLE_TEXTURE_ID.to_string(), built_in_circle_texture());
        this
    }

    // -- Raw texture bytes (RGBA8) --

    /// Store decoded RGBA8 pixel data under a texture ID.
    pub fn insert_texture_bytes(&mut self, id: String, data: RawTextureData) {
        #[cfg(feature = "gpu-wgpu")]
        self.mip_cache.remove(&id);
        *self.texture_revisions.entry(id.clone()).or_default() += 1;
        self.texture_bytes.insert(id, data);
    }

    /// A number that changes whenever a texture's cached pixels do - when
    /// they are inserted, and each [`Self::mark_texture_dirty`]. A reader that
    /// keeps something worked out from a texture's pixels (the points a
    /// `sprite` emit shape emits from) compares it rather than the pixels:
    /// a texture painted at runtime keeps its id and changes underneath.
    pub fn texture_revision(&self, id: &str) -> u64 {
        self.texture_revisions.get(id).copied().unwrap_or(0)
    }

    /// Get a reference to the cached raw RGBA8 data for a texture.
    pub fn get_texture_bytes(&self, id: &str) -> Option<&RawTextureData> {
        self.texture_bytes.get(id)
    }

    /// The cached pixels of a texture, to change in place. Tell the GPU copy
    /// with [`Self::mark_texture_dirty`].
    pub fn texture_bytes_mut(&mut self, id: &str) -> Option<&mut RawTextureData> {
        self.texture_bytes.get_mut(id)
    }

    /// Say that the rectangle `(x, y, w, h)` of a texture's cached pixels has
    /// changed: the next [`Self::ensure_wgpu_texture`] writes just that
    /// rectangle to the uploaded texture, and patches the same place in its
    /// mipmaps if it has them. Every host's draw goes through that call, so a
    /// texture changed at runtime needs no wiring in any of them.
    ///
    /// Mark each edit as its own rectangle: rectangles that touch are merged,
    /// ones apart are kept apart (up to [`Self::MAX_DIRTY_RECTS`] a texture,
    /// past which they become one), so two skids at either end of a floor
    /// cost two small writes, not one the size of the floor.
    pub fn mark_texture_dirty(&mut self, id: &str, x: u32, y: u32, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        *self.texture_revisions.entry(id.to_string()).or_default() += 1;
        let list = self.dirty_rects.entry(id.to_string()).or_default();
        // End-exclusive while merging.
        let mut r = (x, y, x + w, y + h);
        loop {
            let mut merged = false;
            list.retain(|&(a, b, c, d)| {
                let touches = a <= r.2 && r.0 <= a + c && b <= r.3 && r.1 <= b + d;
                if touches {
                    r = (r.0.min(a), r.1.min(b), r.2.max(a + c), r.3.max(b + d));
                    merged = true;
                }
                !touches
            });
            if !merged {
                break;
            }
        }
        list.push((r.0, r.1, r.2 - r.0, r.3 - r.1));
        if list.len() > Self::MAX_DIRTY_RECTS {
            let (x0, y0) = list.iter().fold((u32::MAX, u32::MAX), |m, q| (m.0.min(q.0), m.1.min(q.1)));
            let (x1, y1) = list.iter().fold((0, 0), |m, q| (m.0.max(q.0 + q.2), m.1.max(q.1 + q.3)));
            *list = vec![(x0, y0, x1 - x0, y1 - y0)];
        }
    }

    /// How many separate changed rectangles a texture keeps before they are
    /// written as one.
    pub const MAX_DIRTY_RECTS: usize = 16;

    /// The rectangles marked changed on a texture and not yet written.
    pub fn dirty_rects(&self, id: &str) -> &[(u32, u32, u32, u32)] {
        self.dirty_rects.get(id).map_or(&[], |v| v.as_slice())
    }

    /// Check whether raw bytes for a texture are cached.
    pub fn has_texture_bytes(&self, id: &str) -> bool {
        self.texture_bytes.contains_key(id)
    }

    /// Iterate over all cached texture ids.
    pub fn texture_ids(&self) -> impl Iterator<Item = &String> {
        self.texture_bytes.keys()
    }

    /// Decode a PNG/JPEG file from disk into the raw-bytes cache. Native only.
    /// Returns `Ok(())` on success, error if the file can't be read or decoded.
    /// No-op if the texture is already cached.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_texture_bytes(&mut self, id: &str, path: &str) -> anyhow::Result<()> {
        if self.texture_bytes.contains_key(id) {
            return Ok(());
        }
        let data = RawTextureData::from_file(path)?;
        self.texture_bytes.insert(id.to_string(), data);
        Ok(())
    }

    /// P1.5 — hot-reload entry point for textures. Drops the cached raw
    /// RGBA bytes *and* any uploaded wgpu texture for `id`, then
    /// re-decodes `path` and reinserts the new raw bytes. The next
    /// `ensure_wgpu_texture` call will reupload to the GPU on demand.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn reload_texture_bytes(&mut self, id: &str, path: &str) -> anyhow::Result<()> {
        self.texture_bytes.remove(id);
        #[cfg(feature = "gpu-wgpu")]
        {
            self.wgpu_textures.remove(id);
        }
        let data = RawTextureData::from_file(path)?;
        self.texture_bytes.insert(id.to_string(), data);
        Ok(())
    }

    /// How textures are sampled (`renderer::texture_filter`): each host
    /// hands over the project's rules when it reads its settings. A change
    /// drops every uploaded texture, so each is uploaded again - with its
    /// new sampler and mipmaps - the next time something draws it.
    pub fn set_texture_filters(&mut self, rules: crate::renderer::texture_filter::TextureFilterRules) {
        if rules == self.texture_filters {
            return;
        }
        self.texture_filters = rules;
        #[cfg(feature = "gpu-wgpu")]
        self.wgpu_textures.clear();
    }

    /// The rules [`Self::set_texture_filters`] last set.
    pub fn texture_filters(&self) -> &crate::renderer::texture_filter::TextureFilterRules {
        &self.texture_filters
    }

    // -- WGPU GPU textures --

    /// Lazily upload a cached raw-bytes texture to the wgpu backend and
    /// return a borrowed handle. On first call for a given `id` this
    /// creates the GPU texture; subsequent calls return the cached handle.
    /// Returns `Err` if no raw bytes are cached for this id.
    /// A camera's target texture (`ecs::views`), made - or remade at a new
    /// size - under `id`, so a sprite naming `id` samples what the camera
    /// drew. True when it was made this call.
    /// Mark a texture as a signed distance field, as the built-in font is
    /// (`renderer::text`): it uploads linear and draws through the sprite
    /// shader's distance branch. Every host marks the built-in font when it
    /// registers it.
    pub fn mark_sdf(&mut self, id: &str) {
        if self.sdf_textures.insert(id.to_string()) {
            #[cfg(feature = "gpu-wgpu")]
            self.wgpu_textures.remove(id);
        }
    }

    #[cfg(feature = "gpu-wgpu")]
    pub fn ensure_render_texture(
        &mut self,
        id: &str,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        device: &wgpu::Device,
    ) -> bool {
        use crate::renderer::backend::wgpu::WgpuTexture;
        let fits = self
            .wgpu_textures
            .get(id)
            .is_some_and(|t| t.width == width.max(1) && t.height == height.max(1) && t.texture.format() == format);
        if fits {
            return false;
        }
        self.wgpu_textures.insert(id.to_string(), WgpuTexture::render_target(device, width, height, format));
        true
    }

    /// Forget a GPU texture, so the next draw naming it uploads it again (or,
    /// for a camera's target that is gone, draws nothing).
    #[cfg(feature = "gpu-wgpu")]
    pub fn remove_wgpu_texture(&mut self, id: &str) {
        self.wgpu_textures.remove(id);
    }

    /// Write the rectangles of `id`'s bytes changed since the GPU copy under
    /// `key` was made ([`Self::mark_texture_dirty`]) - and the mip levels they
    /// reach - rather than upload it whole. A copy whose size changed is
    /// dropped instead, for its caller to upload again. Colour textures and
    /// normal maps (a pressed snowfield's) both come through here.
    #[cfg(feature = "gpu-wgpu")]
    fn patch_dirty(&mut self, id: &str, key: &str, queue: &wgpu::Queue) {
        if !self.dirty_rects.is_empty()
            && let Some(rects) = self.dirty_rects.remove(id)
            && let Some(tex) = self.wgpu_textures.get(key)
            && let Some(raw) = self.texture_bytes.get(id)
        {
            if tex.texture.width() != raw.width || tex.texture.height() != raw.height {
                // An image that grew (a font file's atlas) needs a texture of
                // its new size: upload it again.
                self.wgpu_textures.remove(key);
                self.mip_cache.remove(key);
            } else {
                let mut writes: Vec<(u32, (u32, u32, u32, u32))> = Vec::new();
                for (x, y, w, h) in rects {
                    let (x, y) = (x.min(raw.width), y.min(raw.height));
                    let (w, h) = (w.min(raw.width - x), h.min(raw.height - y));
                    if w == 0 || h == 0 {
                        continue;
                    }
                    writes.push((0, (x, y, w, h)));
                    let levels = tex.texture.mip_level_count();
                    if levels > 1 {
                        // Patch the mip chain where the change reaches, rather
                        // than rebuild it whole: a texture painted every frame
                        // (a racing game's skid marks on an 884-texel floor) spent
                        // most of the editor's frame averaging every texel of it.
                        let srgb = tex.texture.format().is_srgb();
                        let chain = self.mip_cache.entry(key.to_string()).or_default();
                        if chain.len() + 1 != levels as usize {
                            *chain = crate::renderer::texture_filter::mip_chain(raw.width, raw.height, &raw.rgba, srgb);
                            writes.extend(chain.iter().enumerate().map(|(k, l)| (k as u32 + 1, (0, 0, l.0, l.1))));
                        } else {
                            writes.extend(crate::renderer::texture_filter::update_mip_chain(
                                &raw.rgba, raw.width, raw.height, chain, (x, y, w, h), srgb,
                            ));
                        }
                    }
                }
                let chain = self.mip_cache.get(key);
                for (level, (rx, ry, rw, rh)) in writes {
                    let (src_w, src) = if level == 0 {
                        (raw.width, raw.rgba.as_slice())
                    } else {
                        let l = &chain.expect("a mip write has a chain")[level as usize - 1];
                        (l.0, l.2.as_slice())
                    };
                    let mut rows = Vec::with_capacity((rw * rh * 4) as usize);
                    for row in ry..ry + rh {
                        let start = ((row * src_w + rx) * 4) as usize;
                        rows.extend_from_slice(&src[start..start + (rw * 4) as usize]);
                    }
                    queue.write_texture(
                        wgpu::ImageCopyTexture {
                            texture: &tex.texture,
                            mip_level: level,
                            origin: wgpu::Origin3d { x: rx, y: ry, z: 0 },
                            aspect: wgpu::TextureAspect::All,
                        },
                        &rows,
                        wgpu::ImageDataLayout { offset: 0, bytes_per_row: Some(rw * 4), rows_per_image: Some(rh) },
                        wgpu::Extent3d { width: rw, height: rh, depth_or_array_layers: 1 },
                    );
                }
            }
        }
    }

    #[cfg(feature = "gpu-wgpu")]
    pub fn ensure_wgpu_texture(
        &mut self,
        id: &str,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> anyhow::Result<&crate::renderer::backend::wgpu::WgpuTexture> {
        use crate::renderer::backend::wgpu::WgpuTexture;
        self.patch_dirty(id, id, queue);
        if !self.wgpu_textures.contains_key(id) {
            let raw = self
                .texture_bytes
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("no raw texture bytes cached for id '{}'", id))?;
            // A distance field is read between texels, so it is linear
            // whatever the project filters - and unmipped
            // (`WgpuTexture::from_rgba_sdf` says why).
            let tex = if self.sdf_textures.contains(id) {
                WgpuTexture::from_rgba_sdf(device, queue, raw.width, raw.height, &raw.rgba)?
            } else {
                let filter = self.texture_filters.filter_for(id);
                upload_filtered(&mut self.mip_generator, device, queue, raw, filter, true)?
            };
            // Uploaded whole: any patched chain kept for it is not this one.
            self.mip_cache.remove(id);
            self.wgpu_textures.insert(id.to_string(), tex);
        }
        Ok(self.wgpu_textures.get(id).expect("just inserted"))
    }

    /// Upload a **normal map** - linear, not sRGB - and return it.
    ///
    /// [`Self::ensure_wgpu_texture`] uploads as sRGB, which is right for colour
    /// and wrong for a normal map: sampling gamma-decodes the stored vectors,
    /// tilting every one of them. Cached under its own key, so an image used
    /// both as a sprite and as a normal map gets two correct uploads rather
    /// than one of them sharing the other's format.
    #[cfg(feature = "gpu-wgpu")]
    pub fn ensure_wgpu_normal_texture(
        &mut self,
        id: &str,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> anyhow::Result<&crate::renderer::backend::wgpu::WgpuTexture> {
        let key = normal_texture_key(id);
        // A normal map changed at runtime (`Sprite.press`) is patched where
        // it changed, as a colour texture is.
        self.patch_dirty(id, &key, queue);
        if !self.wgpu_textures.contains_key(&key) {
            let raw = self
                .texture_bytes
                .get(id)
                .ok_or_else(|| anyhow::anyhow!("no raw texture bytes cached for normal map '{}'", id))?;
            // Filtered like the colour it goes with: a smooth sprite with a
            // point-sampled normal map lights in blocks.
            let filter = self.texture_filters.filter_for(id);
            let tex = upload_filtered(&mut self.mip_generator, device, queue, raw, filter, false)?;
            self.mip_cache.remove(&key);
            self.wgpu_textures.insert(key.clone(), tex);
        }
        Ok(self.wgpu_textures.get(&key).expect("just inserted"))
    }

    /// Borrow a normal map uploaded by [`Self::ensure_wgpu_normal_texture`].
    #[cfg(feature = "gpu-wgpu")]
    pub fn get_wgpu_normal_texture(&self, id: &str) -> Option<&crate::renderer::backend::wgpu::WgpuTexture> {
        self.wgpu_textures.get(&normal_texture_key(id))
    }

    /// Borrow a previously-uploaded wgpu texture without materializing a
    /// new one. Returns `None` if `ensure_wgpu_texture` hasn't been
    /// called for this id yet - **or if its pixels changed since**
    /// ([`Self::mark_texture_dirty`]): a stale copy is not handed out.
    ///
    /// That second case is what makes a texture changed at runtime need no
    /// wiring in any host. Every host's flush asks this first and calls
    /// [`Self::ensure_wgpu_texture`] only on `None` - sixteen sites across the
    /// three of them - so the first version of carving wrote its holes into
    /// the bytes, queued the GPU write, and no host ever asked for it: the
    /// island stayed whole on screen while its collider had craters.
    #[cfg(feature = "gpu-wgpu")]
    pub fn get_wgpu_texture(&self, id: &str) -> Option<&crate::renderer::backend::wgpu::WgpuTexture> {
        if !self.dirty_rects.is_empty() && self.dirty_rects.contains_key(id) {
            return None;
        }
        self.wgpu_textures.get(id)
    }

    /// Drop every cached wgpu texture, freeing the underlying GPU memory.
    /// Useful for hot-reload / scene unload paths. Raw RGBA bytes in
    /// `texture_bytes` are kept so the next sprite draw can re-upload via
    /// [`Self::ensure_wgpu_texture`].
    #[cfg(feature = "gpu-wgpu")]
    pub fn destroy_wgpu_textures(&mut self) {
        self.wgpu_textures.clear();
        self.mip_generator = None;
    }

    // -- Atlas metadata --

    /// Store atlas metadata under the atlas id.
    pub fn insert_atlas_data(&mut self, id: String, data: AtlasData) {
        self.atlas_data.insert(id, data);
    }

    /// Register a loaded atlas: its metadata under `id`, and a mapping from
    /// every region name to it for `atlas_lookup_entry` / `get_sprite_region`.
    ///
    /// The one way every runtime registers an atlas. The native scene loader
    /// did both halves while the editor viewport stored only the metadata, so
    /// region-name lookups worked in a build and returned nothing in the
    /// editor.
    pub fn register_atlas(&mut self, id: String, data: AtlasData) {
        for region in &data.regions {
            self.atlas_lookup
                .insert(region.name.clone(), (id.clone(), region.name.clone()));
        }
        self.atlas_data.insert(id, data);
    }

    /// Get atlas metadata by atlas id.
    pub fn get_atlas_data(&self, id: &str) -> Option<&AtlasData> {
        self.atlas_data.get(id)
    }

    /// Check if atlas metadata is loaded.
    pub fn has_atlas_data(&self, id: &str) -> bool {
        self.atlas_data.contains_key(id)
    }

    /// Iterate over all atlas ids.
    pub fn atlas_ids(&self) -> impl Iterator<Item = &String> {
        self.atlas_data.keys()
    }

    // -- Bitmap fonts --

    /// Store a bitmap font under its font id (the `.font` asset path).
    ///
    /// The caller is responsible for loading the matching atlas image into
    /// the texture cache under [`font_texture_id(id)`](font_texture_id) -
    /// [`crate::renderer::font_asset::load_fonts`] does both together.
    ///
    /// A distance-field font (`"sdf": true`) marks that image as one here, so
    /// the sprite shader draws it as an edge rather than as coverage. Every
    /// host loads a font through this, which is why none of them marks a
    /// custom font itself; before this only the built-in one was marked, by
    /// each host by name.
    pub fn insert_font_data(&mut self, id: String, data: FontData) {
        if data.sdf {
            self.mark_sdf(&crate::renderer::font_asset::font_texture_id(&id));
        }
        self.fonts.insert(id, Arc::new(data));
    }

    /// Get a bitmap font by font id.
    pub fn get_font_data(&self, id: &str) -> Option<&FontData> {
        self.fonts.get(id).map(|f| f.as_ref())
    }

    /// Share a bitmap font without borrowing the manager.
    ///
    /// The render path needs the frame's fonts to outlive the `&AssetManager`
    /// borrow, because the flush closures capture `&mut` of the same manager.
    pub fn get_font_shared(&self, id: &str) -> Option<Arc<FontData>> {
        self.fonts.get(id).map(Arc::clone)
    }

    /// Check whether a bitmap font is loaded.
    pub fn has_font_data(&self, id: &str) -> bool {
        self.fonts.contains_key(id)
    }

    /// Iterate over all loaded font ids.
    pub fn font_ids(&self) -> impl Iterator<Item = &String> {
        self.fonts.keys()
    }

    /// Register a mapping from an original sprite path to an atlas region.
    pub fn register_atlas_mapping(&mut self, original_path: String, atlas_id: String, region_name: String) {
        self.atlas_lookup.insert(original_path, (atlas_id, region_name));
    }

    /// Iterate over all atlas lookup entries.
    pub fn atlas_mappings(&self) -> impl Iterator<Item = (&String, &(String, String))> {
        self.atlas_lookup.iter()
    }

    /// Get the atlas mapping for a sprite path (if it exists).
    pub fn atlas_lookup_entry(&self, texture_id: &str) -> Option<(String, String)> {
        self.atlas_lookup.get(texture_id).cloned()
    }

    /// Get the full atlas region for a sprite (with anchor and points).
    pub fn get_sprite_region(&self, texture_id: &str) -> Option<&AtlasRegion> {
        if let Some((atlas_id, region_name)) = self.atlas_lookup.get(texture_id)
            && let Some(atlas) = self.atlas_data.get(atlas_id.as_str())
        {
            return atlas.get_region(region_name);
        }
        None
    }

    /// Build `AnimationClip`s from atlas animation clips for a given atlas.
    ///
    /// Returns the atlas texture ID and a map of clip_name → AnimationClip
    /// that can be inserted into a `SpriteAnimator`.
    pub fn build_animation_clips_from_atlas(
        &self,
        atlas_id: &str,
    ) -> Option<HashMap<String, AnimationClip>> {
        let atlas = self.atlas_data.get(atlas_id)?;
        if atlas.animation_clips.is_empty() {
            return None;
        }
        let mut clips = HashMap::new();
        for clip in &atlas.animation_clips {
            let mut frames = Vec::new();
            for frame in &clip.frames {
                if let Some(uv_rect) = atlas.get_uv_rect(&frame.region_name) {
                    frames.push(AnimationFrame { uv_rect, region: None, texture_id: None, triggers: Vec::new() });
                }
            }
            if !frames.is_empty() {
                clips.insert(
                    clip.name.clone(),
                    AnimationClip {
                        name: clip.name.clone(),
                        frames,
                        frame_duration: if clip.fps > 0.0 {
                            1.0 / clip.fps
                        } else {
                            0.1
                        },
                        looping: clip.looping,
                        property_tracks: Vec::new(),
                    },
                );
            }
        }
        Some(clips)
    }

    /// Get the resolved points for a specific animation frame within an atlas.
    /// Whether any atlas holds a named point - on a region, or on a frame of
    /// one of its clips. When none does, no sprite has a point to answer
    /// `Sprite.get_point` with, and the per-frame point table
    /// (`scripting::api::compute_sprite_points`) need not walk the sprites at
    /// all. A walk of the atlases, which are few.
    pub fn any_sprite_points(&self) -> bool {
        self.atlas_data.values().any(|a| {
            a.regions.iter().any(|r| !r.points.is_empty())
                || a.animation_clips.iter().any(|c| c.frames.iter().any(|f| !f.points.is_empty()))
        })
    }

    pub fn get_animation_frame_points(
        &self,
        atlas_id: &str,
        clip_name: &str,
        frame_index: usize,
    ) -> Option<&[crate::renderer::atlas::SpritePoint]> {
        let atlas = self.atlas_data.get(atlas_id)?;
        let clip = atlas
            .animation_clips
            .iter()
            .find(|c| c.name == clip_name)?;
        let frame = clip.frames.get(frame_index)?;
        Some(&frame.points)
    }

    // -- Audio assets --

    /// Store raw audio data under a clip ID.
    pub fn insert_audio(&mut self, id: String, data: Vec<u8>) {
        self.audio_clips.insert(id, data);
    }

    /// Get raw audio data by clip ID.
    pub fn get_audio_data(&self, id: &str) -> Option<&[u8]> {
        self.audio_clips.get(id).map(|v| v.as_slice())
    }

    /// Check if an audio clip is loaded.
    pub fn has_audio(&self, id: &str) -> bool {
        self.audio_clips.contains_key(id)
    }

    // -- GUI document assets --

    /// Store a GUI document under an asset ID.
    pub fn insert_gui(&mut self, id: String, doc: GuiDocument) {
        self.gui_documents.insert(id, doc);
    }

    /// Get a reference to a GUI document by its ID.
    pub fn get_gui(&self, id: &str) -> Option<&GuiDocument> {
        self.gui_documents.get(id)
    }

    /// Get a mutable reference to a GUI document by its ID.
    pub fn get_gui_mut(&mut self, id: &str) -> Option<&mut GuiDocument> {
        self.gui_documents.get_mut(id)
    }

    /// Check if a GUI document is loaded.
    pub fn has_gui(&self, id: &str) -> bool {
        self.gui_documents.contains_key(id)
    }

    // -- Material assets --

    /// Store a `MaterialData` under an asset ID.
    pub fn insert_material(&mut self, id: String, data: MaterialData) {
        note_material_graph(&id, &data);
        self.materials.insert(id, data);
    }

    /// Get a material by its ID.
    pub fn get_material(&self, id: &str) -> Option<&MaterialData> {
        self.materials.get(id)
    }

    /// Check if a material is cached.
    pub fn has_material(&self, id: &str) -> bool {
        self.materials.contains_key(id)
    }

    /// Iterate over registered material IDs.
    pub fn material_ids(&self) -> impl Iterator<Item = &String> {
        self.materials.keys()
    }

    // -- Skeletal animation clips (slice 3.5.C) --

    /// Register a [`SkeletalClip`] under an asset id (typically the
    /// `.skelanim` file's relative path).
    pub fn insert_skeletal_clip(&mut self, id: String, clip: SkeletalClip) {
        self.skeletal_clips.insert(id, clip);
    }

    /// Borrow a skeletal clip by id.
    pub fn get_skeletal_clip(&self, id: &str) -> Option<&SkeletalClip> {
        self.skeletal_clips.get(id)
    }

    /// True if a skeletal clip is cached under `id`.
    pub fn has_skeletal_clip(&self, id: &str) -> bool {
        self.skeletal_clips.contains_key(id)
    }

    /// Borrow the entire clip map. Convenient for passing into
    /// [`crate::animation::skeletal::update_skeletal_players`].
    pub fn skeletal_clips(&self) -> &HashMap<String, SkeletalClip> {
        &self.skeletal_clips
    }

    /// Iterate over registered skeletal-clip ids.
    pub fn skeletal_clip_ids(&self) -> impl Iterator<Item = &String> {
        self.skeletal_clips.keys()
    }

    /// Load every `.skelanim` file under `dir` (recursively), keyed by its
    /// path relative to `dir` — `animations/walk.skelanim` for a file at
    /// `<dir>/animations/walk.skelanim`, which is the id a scene's
    /// `SkeletalPlayer.clip` names.
    ///
    /// Returns the number of clips loaded. Runtimes call this at scene load;
    /// without it nothing ever populated the clip map, so `SkeletalPlayer`
    /// held its pose forever with no way to author a clip that would play.
    ///
    /// Native only — the WASM builds have no filesystem and are handed clips
    /// by their host instead.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_skeletal_clips_from_dir(&mut self, dir: &std::path::Path) -> usize {
        self.load_each_under(dir, "skelanim", "skeletal clip", Self::load_skeletal_clip)
    }

    /// Load every `.rig` file under `dir` (recursively), keyed by its path
    /// relative to `dir` - `rigs/arm.rig` - which is the id
    /// `Skeleton.spawn_rig` names. A `.rig` is a [`SkeletonRig`] as JSON.
    ///
    /// Until Oct 2026 nothing loaded rigs in any runtime, so `spawn_rig` bound
    /// everywhere and spawned nothing anywhere. Native only, as clips are; the
    /// browsers are handed rigs by their host.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_skeleton_rigs_from_dir(&mut self, dir: &std::path::Path) -> usize {
        self.load_each_under(dir, "rig", "skeleton rig", Self::load_skeleton_rig)
    }

    /// Call `load(id, path)` for every file under `dir` with extension `ext`,
    /// `id` being its `/`-separated path relative to `dir`. Returns how many
    /// loaded; a failure is logged and skipped.
    #[cfg(not(target_arch = "wasm32"))]
    fn load_each_under(
        &mut self,
        dir: &std::path::Path,
        ext: &str,
        what: &str,
        load: fn(&mut Self, &str, &str) -> anyhow::Result<()>,
    ) -> usize {
        fn walk(
            am: &mut AssetManager,
            dir: &std::path::Path,
            root: &std::path::Path,
            ext: &str,
            what: &str,
            load: fn(&mut AssetManager, &str, &str) -> anyhow::Result<()>,
        ) -> usize {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return 0;
            };
            let mut n = 0;
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    n += walk(am, &path, root, ext, what, load);
                    continue;
                }
                if path.extension().and_then(|e| e.to_str()) != Some(ext) {
                    continue;
                }
                let id = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .replace('\\', "/");
                match load(am, &id, &path.to_string_lossy()) {
                    Ok(()) => n += 1,
                    Err(e) => log::warn!("skipping {what} {}: {}", path.display(), e),
                }
            }
            n
        }
        walk(self, dir, dir, ext, what, load)
    }

    /// Load a `.skelanim` JSON file from disk and cache it under `id`.
    /// Native only. No-op if a clip with that id is already cached.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_skeletal_clip(&mut self, id: &str, path: &str) -> anyhow::Result<()> {
        if self.skeletal_clips.contains_key(id) {
            return Ok(());
        }
        let bytes = std::fs::read(path)?;
        let clip: SkeletalClip = serde_json::from_slice(&bytes)?;
        self.skeletal_clips.insert(id.to_string(), clip);
        Ok(())
    }

    // -- Skeleton rigs (slice 3.5.D) --

    /// Register a [`SkeletonRig`] under an asset id.
    pub fn insert_skeleton_rig(&mut self, id: String, rig: SkeletonRig) {
        self.skeleton_rigs.insert(id, rig);
    }

    /// Borrow a skeleton rig by id.
    pub fn get_skeleton_rig(&self, id: &str) -> Option<&SkeletonRig> {
        self.skeleton_rigs.get(id)
    }

    /// True if a skeleton rig is cached under `id`.
    pub fn has_skeleton_rig(&self, id: &str) -> bool {
        self.skeleton_rigs.contains_key(id)
    }

    /// Iterate over registered skeleton-rig ids.
    pub fn skeleton_rig_ids(&self) -> impl Iterator<Item = &String> {
        self.skeleton_rigs.keys()
    }

    /// Load a `.rig` JSON file (homegrown [`SkeletonRig`] format) and
    /// cache it under `id`. Native only. No-op if already cached.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_skeleton_rig(&mut self, id: &str, path: &str) -> anyhow::Result<()> {
        if self.skeleton_rigs.contains_key(id) {
            return Ok(());
        }
        let bytes = std::fs::read(path)?;
        let rig: SkeletonRig = serde_json::from_slice(&bytes)?;
        self.skeleton_rigs.insert(id.to_string(), rig);
        Ok(())
    }

    /// Import a Spine-format JSON skeleton file: registers the rig
    /// under `rig_id` and every contained animation under
    /// `"{clip_id_prefix}{anim_name}"`. Returns the list of clip ids
    /// that were registered. Native only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_spine_skeleton(
        &mut self,
        rig_id: &str,
        clip_id_prefix: &str,
        path: &str,
    ) -> anyhow::Result<Vec<String>> {
        let json = std::fs::read_to_string(path)?;
        self.import_spine_skeleton(rig_id, clip_id_prefix, &json)
    }

    /// Parse a Spine JSON string and register the rig + clips. Works
    /// on every platform (used by tests + WASM-side loading).
    pub fn import_spine_skeleton(
        &mut self,
        rig_id: &str,
        clip_id_prefix: &str,
        json: &str,
    ) -> anyhow::Result<Vec<String>> {
        let (rig, clips) = crate::animation::rig::parse_spine_json(json)?;
        self.skeleton_rigs.insert(rig_id.to_string(), rig);
        let mut ids = Vec::with_capacity(clips.len());
        for clip in clips {
            let id = format!("{clip_id_prefix}{}", clip.name);
            self.skeletal_clips.insert(id.clone(), clip);
            ids.push(id);
        }
        Ok(ids)
    }

    /// Load a `.material` JSON file from disk and cache it under `id`. Native only.
    /// No-op if a material with the given ID is already cached.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_material(&mut self, id: &str, path: &str) -> anyhow::Result<()> {
        if self.materials.contains_key(id) {
            return Ok(());
        }
        let json = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read material file '{path}': {e}"))?;
        let data: MaterialData = serde_json::from_str(&json)
            .map_err(|e| anyhow::anyhow!("failed to parse material '{path}': {e}"))?;
        note_material_graph(id, &data);
        self.materials.insert(id.to_string(), data);
        Ok(())
    }

    /// Parse a `.material` JSON string and cache it under `id`. Works on all
    /// platforms (used by the web runtime to feed pre-fetched material bytes).
    pub fn load_material_from_str(&mut self, id: &str, json: &str) -> anyhow::Result<()> {
        if self.materials.contains_key(id) {
            return Ok(());
        }
        let data: MaterialData = serde_json::from_str(json)
            .map_err(|e| anyhow::anyhow!("failed to parse material '{id}': {e}"))?;
        note_material_graph(id, &data);
        self.materials.insert(id.to_string(), data);
        Ok(())
    }

    // -- Sort layers (`.sortlayers`) --

    /// Insert a pre-parsed [`SortLayers`] under `id`. Overwrites any
    /// existing set with the same id.
    pub fn insert_sort_layers(&mut self, id: String, layers: SortLayers) {
        self.sort_layer_sets.insert(id, layers);
    }

    /// Parse a `.sortlayers` JSON string and cache it under `id`. Works
    /// on all platforms.
    pub fn load_sort_layers_from_str(&mut self, id: &str, json: &str) -> anyhow::Result<()> {
        let layers = SortLayers::from_json(json)
            .map_err(|e| anyhow::anyhow!("failed to parse sort layers '{id}': {e}"))?;
        self.sort_layer_sets.insert(id.to_string(), layers);
        Ok(())
    }

    /// Load a `.sortlayers` JSON file from disk and cache it under `id`.
    /// Native only.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn load_sort_layers(&mut self, id: &str, path: &str) -> anyhow::Result<()> {
        let json = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("failed to read sort layers '{path}': {e}"))?;
        self.load_sort_layers_from_str(id, &json)
    }

    /// Get a cached sort-layer set by id.
    pub fn get_sort_layers(&self, id: &str) -> Option<&SortLayers> {
        self.sort_layer_sets.get(id)
    }

    /// Iterate over registered sort-layer set IDs.
    pub fn sort_layer_set_ids(&self) -> impl Iterator<Item = &String> {
        self.sort_layer_sets.keys()
    }

    /// Set the active sort-layer set (scene loaders call this with the
    /// scene's `sort_layers` field). Pass `None` to clear.
    pub fn set_active_sort_layers(&mut self, id: Option<String>) {
        self.active_sort_layers = id;
    }

    /// Id of the currently active sort-layer set, if any.
    pub fn active_sort_layers_id(&self) -> Option<&str> {
        self.active_sort_layers.as_deref()
    }

    /// Borrow the active [`SortLayers`] set; returns an empty fallback
    /// when no set is active so callers can always resolve without an
    /// `Option` dance.
    pub fn active_sort_layers(&self) -> &SortLayers {
        match self.active_sort_layers.as_deref() {
            Some(id) => self
                .sort_layer_sets
                .get(id)
                .unwrap_or(&self.empty_sort_layers),
            None => &self.empty_sort_layers,
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// P1.5 — `reload_texture_bytes` must drop the raw cache entry
    /// (and any uploaded wgpu texture under the same id) and decode
    /// the file fresh, so live-edits to a PNG show up next frame.
    #[test]
    fn reload_texture_bytes_replaces_raw_cache() {
        use image::{ImageBuffer, Rgba};

        fn write_solid(path: &std::path::Path, color: [u8; 4]) {
            let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
                ImageBuffer::from_pixel(2, 2, Rgba(color));
            img.save(path).expect("encode png");
        }

        let dir = std::env::temp_dir().join(format!(
            "jengine-asset-reload-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("mkdir");
        let path = dir.join("tex.png");
        write_solid(&path, [255, 0, 0, 255]); // red

        let mut assets = AssetManager::new();
        assets
            .load_texture_bytes("tex.png", path.to_str().unwrap())
            .expect("initial load");
        let bytes_v1 = assets.get_texture_bytes("tex.png").expect("has v1").rgba.clone();
        assert_eq!(&bytes_v1[..4], &[255, 0, 0, 255]);

        // Overwrite the file and call the no-op variant first to prove the
        // cache stays stale — that's the bug `reload_texture_bytes` fixes.
        write_solid(&path, [0, 255, 0, 255]); // green
        assets
            .load_texture_bytes("tex.png", path.to_str().unwrap())
            .expect("noop reload");
        let bytes_after_noop = assets.get_texture_bytes("tex.png").expect("still v1").rgba.clone();
        assert_eq!(
            bytes_after_noop, bytes_v1,
            "load_texture_bytes must NOT bust the cache (its contract is no-op-if-present)"
        );

        // reload_texture_bytes must drop the cache and re-decode.
        assets
            .reload_texture_bytes("tex.png", path.to_str().unwrap())
            .expect("hot reload");
        let bytes_v2 = assets.get_texture_bytes("tex.png").expect("has v2").rgba.clone();
        assert_eq!(&bytes_v2[..4], &[0, 255, 0, 255]);
        assert_ne!(bytes_v1, bytes_v2);

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}

/// The cache key a normal map's linear upload lives under.
///
/// Distinct from the plain id so the same image can be uploaded both as sRGB
/// colour and as linear data without one upload answering for the other.
#[cfg(feature = "gpu-wgpu")]
fn normal_texture_key(id: &str) -> String {
    format!("{id}#normal")
}

/// Texture ids a renderer found missing that a runtime should now load.
///
/// Every runtime preloads the textures its scene names, but a sprite can
/// arrive later with a texture nothing preloaded: a package creating its own
/// sprites (a match-3 package's gems), a prefab, an animation frame. The renderers
/// already notice - each keeps a set of ids it dropped draws for - so this
/// turns that set into requests: ids with no bytes cached yet, each asked for
/// once. Engine built-ins (`__white` and friends) are never requested; a
/// failed request is not retried.
pub fn textures_to_request(
    missing: &HashSet<String>,
    requested: &mut HashSet<String>,
    assets: &AssetManager,
) -> Vec<String> {
    let mut out: Vec<String> = missing
        .iter()
        .filter(|id| !id.starts_with("__") && !id.is_empty())
        .filter(|id| !assets.has_texture_bytes(id) && !requested.contains(*id))
        .cloned()
        .collect();
    out.sort();
    requested.extend(out.iter().cloned());
    out
}

#[cfg(test)]
mod texture_request_tests {
    use super::*;

    /// Two edits far apart stay two rectangles; edits that touch become one;
    /// past the limit they are written as one.
    #[test]
    fn dirty_rectangles_merge_only_when_they_touch() {
        let mut am = AssetManager::new();
        am.mark_texture_dirty("t", 0, 0, 4, 4);
        am.mark_texture_dirty("t", 800, 800, 4, 4);
        assert_eq!(am.dirty_rects("t"), &[(0, 0, 4, 4), (800, 800, 4, 4)]);
        // Touching the first one's edge joins it.
        am.mark_texture_dirty("t", 4, 2, 3, 3);
        assert_eq!(am.dirty_rects("t").len(), 2);
        assert!(am.dirty_rects("t").contains(&(0, 0, 7, 5)));
        // A rectangle bridging both swallows them.
        am.mark_texture_dirty("t", 5, 4, 800, 797);
        assert_eq!(am.dirty_rects("t"), &[(0, 0, 805, 804)]);
        // Too many apart: one.
        let mut am = AssetManager::new();
        for k in 0..(AssetManager::MAX_DIRTY_RECTS as u32 + 1) {
            am.mark_texture_dirty("t", k * 10, 0, 2, 2);
        }
        assert_eq!(am.dirty_rects("t"), &[(0, 0, AssetManager::MAX_DIRTY_RECTS as u32 * 10 + 2, 2)]);
        am.mark_texture_dirty("u", 0, 0, 0, 5);
        assert!(am.dirty_rects("u").is_empty(), "an empty rectangle is not a change");
    }

    #[test]
    fn each_missing_texture_is_requested_once_and_built_ins_never() {
        let mut assets = AssetManager::new();
        assets.insert_texture_bytes("have.png".into(), RawTextureData { width: 1, height: 1, rgba: vec![0; 4] });
        let missing: HashSet<String> =
            ["gem.png", "__white", "have.png", "b.png"].iter().map(|s| s.to_string()).collect();
        let mut requested = HashSet::new();
        assert_eq!(textures_to_request(&missing, &mut requested, &assets), vec!["b.png", "gem.png"]);
        assert!(textures_to_request(&missing, &mut requested, &assets).is_empty(), "asked once");
    }
}

/// Tell the sprite pass which graph a material draws with, so it can pack a
/// material instance's `uniforms` into the slots that graph reads (see
/// `renderer::material_params`). Every way a material arrives goes through
/// here.
fn note_material_graph(id: &str, data: &crate::renderer::material::MaterialData) {
    if let Some(path) = data.shader_graph_path.as_deref() {
        crate::renderer::material_params::note_material(id, path);
    }
    // The file's own parameter values: every wearer's defaults.
    crate::renderer::material_params::note_material_defaults(id, &data.uniforms);
}
