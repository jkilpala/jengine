//! The browser's video decoder: an HTML `<video>` element, shared by the
//! editor viewport and web builds. The element plays the file itself - the
//! browser decodes, and keeps the time - and each time it shows a new frame
//! the frame is drawn onto a canvas and its pixels handed to the video's
//! texture. The element is muted: video is silent (`video` module docs).
//!
//! Its URL comes from a resolver the host gives, asked each update until it
//! answers: a web build knows at once (`assets/...`), the editor only once
//! it has read the file and handed the bytes over as a Blob URL.

use wasm_bindgen::JsCast;

use super::{PlayState, VideoDecoder, VideoFrame};

/// Where a video's file can be loaded from, or `None` while it is not yet.
pub type UrlResolver = Box<dyn Fn() -> Option<String>>;

pub struct WebVideoDecoder {
    resolve: UrlResolver,
    element: Option<web_sys::HtmlVideoElement>,
    canvas: Option<(web_sys::HtmlCanvasElement, web_sys::CanvasRenderingContext2d)>,
    /// The element's time when the last frame was taken.
    last_time: f64,
    pending_seek: Option<f64>,
}

impl WebVideoDecoder {
    pub fn new(resolve: UrlResolver) -> Self {
        Self { resolve, element: None, canvas: None, last_time: -1.0, pending_seek: None }
    }

    fn open(&mut self) -> Option<&web_sys::HtmlVideoElement> {
        if self.element.is_none() {
            let url = (self.resolve)()?;
            let document = web_sys::window()?.document()?;
            let el: web_sys::HtmlVideoElement = document.create_element("video").ok()?.dyn_into().ok()?;
            el.set_muted(true);
            el.set_attribute("playsinline", "").ok();
            el.set_attribute("muted", "").ok();
            el.set_cross_origin(Some("anonymous"));
            el.set_preload("auto");
            el.set_src(&url);
            el.load();
            self.element = Some(el);
        }
        self.element.as_ref()
    }
}

impl VideoDecoder for WebVideoDecoder {
    fn update(&mut self, _dt: f64, state: PlayState) -> Option<VideoFrame> {
        let pending = self.pending_seek;
        let el = self.open()?.clone();
        // HAVE_CURRENT_DATA: there is a frame to take.
        if el.ready_state() < 2 {
            return None;
        }
        if let Some(t) = pending {
            el.set_current_time(t);
            self.pending_seek = None;
        }
        el.set_loop(state.looping);
        el.set_playback_rate(state.speed.max(0.0625) as f64);
        if state.playing && el.paused() && !(el.ended() && !state.looping) {
            // A promise; an autoplay refusal is impossible for a muted video.
            let _ = el.play();
        } else if !state.playing && !el.paused() {
            let _ = el.pause();
        }
        let t = el.current_time();
        if (t - self.last_time).abs() < 1e-4 && self.canvas.is_some() {
            return None;
        }
        let (w, h) = (el.video_width(), el.video_height());
        if w == 0 || h == 0 {
            return None;
        }
        if self.canvas.as_ref().is_none_or(|(c, _)| c.width() != w || c.height() != h) {
            let document = web_sys::window()?.document()?;
            let canvas: web_sys::HtmlCanvasElement = document.create_element("canvas").ok()?.dyn_into().ok()?;
            canvas.set_width(w);
            canvas.set_height(h);
            let opts = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&opts, &"willReadFrequently".into(), &true.into());
            let ctx: web_sys::CanvasRenderingContext2d =
                canvas.get_context_with_context_options("2d", &opts).ok()??.dyn_into().ok()?;
            self.canvas = Some((canvas, ctx));
        }
        let (_, ctx) = self.canvas.as_ref()?;
        ctx.draw_image_with_html_video_element(&el, 0.0, 0.0).ok()?;
        let data = ctx.get_image_data(0.0, 0.0, w as f64, h as f64).ok()?;
        self.last_time = t;
        Some(VideoFrame { width: w, height: h, rgba: data.data().0 })
    }

    fn seek(&mut self, t: f64) {
        self.pending_seek = Some(t.max(0.0));
        self.last_time = -1.0;
    }

    fn ended(&self) -> bool {
        self.element.as_ref().is_some_and(|el| el.ended())
    }
}

impl Drop for WebVideoDecoder {
    fn drop(&mut self) {
        if let Some(el) = &self.element {
            let _ = el.pause();
            el.remove_attribute("src").ok();
            el.load();
        }
    }
}
