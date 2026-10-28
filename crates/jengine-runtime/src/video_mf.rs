//! Windows' video decoder for `jengine_core::video`: Media Foundation.
//!
//! The OS decodes (H.264, HEVC, VP9 ... whatever the machine has codecs
//! for, hardware where it can), so the engine carries no codec and no
//! licence. A **thread of its own** owns the source reader and decodes ahead
//! into a small queue; the game's side keeps the clock and shows the newest
//! frame that is due - so a frame never waits on the decoder, and decoding
//! never waits on a frame. A seek or a loop starts a new **generation**:
//! frames of an older one are dropped, frames of a newer one wait their turn.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, SyncSender, TryRecvError, channel, sync_channel};

use jengine_core::video::{PlayState, VideoDecoder, VideoFrame};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0, PropVariantToUInt64};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Variant::VT_I8;
use windows::core::{GUID, HSTRING};

/// A position for `SetCurrentPosition`: 100 ns units, as a VT_I8.
fn position(t: f64) -> PROPVARIANT {
    PROPVARIANT {
        Anonymous: PROPVARIANT_0 {
            Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 {
                vt: VT_I8,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: PROPVARIANT_0_0_0 { hVal: (t * 1e7) as i64 },
            }),
        },
    }
}

enum Cmd {
    Seek { t: f64, generation: u32 },
    Loop(bool),
}

enum Msg {
    Opened { duration: f64 },
    Frame { generation: u32, pts: f64, frame: VideoFrame },
    /// The stream ran out (and does not loop).
    End { generation: u32 },
    Failed(String),
}

/// How many decoded frames may wait in the channel.
const AHEAD: usize = 4;
/// How many the game side holds, besides.
const HELD: usize = 4;

pub struct MfDecoder {
    rx: Receiver<Msg>,
    tx: Sender<Cmd>,
    clock: f64,
    generation: u32,
    duration: f64,
    pending: VecDeque<(u32, f64, VideoFrame)>,
    ended: bool,
    looping: Option<bool>,
    failed: bool,
}

impl MfDecoder {
    /// Start decoding `path`. Failures arrive later, from the thread, and
    /// leave the texture black with a log line.
    pub fn open(path: PathBuf) -> Self {
        let (tx, cmd_rx) = channel();
        let (msg_tx, rx) = sync_channel(AHEAD);
        let name = path.display().to_string();
        std::thread::Builder::new()
            .name(format!("video {name}"))
            .spawn(move || decode_thread(path, cmd_rx, msg_tx))
            .ok();
        Self {
            rx,
            tx,
            clock: 0.0,
            generation: 0,
            duration: 0.0,
            pending: VecDeque::new(),
            ended: false,
            looping: None,
            failed: false,
        }
    }

    fn drain(&mut self) {
        while self.pending.len() < HELD {
            match self.rx.try_recv() {
                Ok(Msg::Opened { duration }) => self.duration = duration,
                Ok(Msg::Frame { generation, pts, frame }) => {
                    if generation >= self.generation {
                        self.pending.push_back((generation, pts, frame));
                    }
                }
                Ok(Msg::End { generation }) => {
                    if generation == self.generation {
                        self.ended = true;
                    }
                }
                Ok(Msg::Failed(e)) => {
                    if !self.failed {
                        log::warn!("video: {e}");
                    }
                    self.failed = true;
                }
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => break,
            }
        }
    }
}

impl VideoDecoder for MfDecoder {
    fn update(&mut self, dt: f64, state: PlayState) -> Option<VideoFrame> {
        if self.looping != Some(state.looping) {
            self.looping = Some(state.looping);
            let _ = self.tx.send(Cmd::Loop(state.looping));
        }
        self.drain();
        let started = !self.pending.is_empty() || self.clock > 0.0;
        if state.playing && started {
            self.clock += dt * state.speed as f64;
        }
        // Round again: the thread has seeked to the start and numbered the
        // frames after it as the next generation.
        if state.looping && self.duration > 0.0 && self.clock >= self.duration {
            self.clock -= self.duration;
            self.generation += 1;
            self.pending.retain(|(g, _, _)| *g >= self.generation);
        }
        if !state.looping && self.duration > 0.0 {
            self.clock = self.clock.min(self.duration);
        }
        // The newest frame that is due; the ones before it are passed.
        let mut shown = None;
        while let Some((g, pts, _)) = self.pending.front() {
            if *g < self.generation {
                self.pending.pop_front();
                continue;
            }
            if *g == self.generation && *pts <= self.clock + 1e-3 {
                shown = self.pending.pop_front().map(|(_, _, f)| f);
                continue;
            }
            break;
        }
        self.drain();
        shown
    }

    fn seek(&mut self, t: f64) {
        self.generation += 1;
        self.clock = t.max(0.0);
        self.ended = false;
        self.pending.clear();
        let _ = self.tx.send(Cmd::Seek { t: self.clock, generation: self.generation });
    }

    fn ended(&self) -> bool {
        self.ended && self.pending.is_empty()
    }
}

/// Decode `path` until the game side goes away, a frame at a time into
/// `out` (which blocks when the game side is far enough behind).
fn decode_thread(path: PathBuf, cmds: Receiver<Cmd>, out: SyncSender<Msg>) {
    unsafe {
        let com = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        if MFStartup(MF_VERSION, MFSTARTUP_FULL).is_err() {
            let _ = out.send(Msg::Failed("Media Foundation did not start".into()));
            return;
        }
        let result = run(&path, &cmds, &out);
        if let Err(e) = result {
            let _ = out.send(Msg::Failed(format!("'{}': {e}", path.display())));
        }
        let _ = MFShutdown();
        if com {
            CoUninitialize();
        }
    }
}

const VIDEO_STREAM: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

unsafe fn run(path: &PathBuf, cmds: &Receiver<Cmd>, out: &SyncSender<Msg>) -> windows::core::Result<()> {
    unsafe {
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 1)?;
        let attrs = attrs.ok_or_else(windows::core::Error::empty)?;
        // Let the reader convert whatever the codec gives into RGB.
        attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)?;
        let url = HSTRING::from(path.as_os_str());
        let reader = MFCreateSourceReaderFromURL(&url, &attrs)?;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader.SetStreamSelection(VIDEO_STREAM, true)?;
        let want = MFCreateMediaType()?;
        want.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        want.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        reader.SetCurrentMediaType(VIDEO_STREAM, None, &want)?;
        let got = reader.GetCurrentMediaType(VIDEO_STREAM)?;
        let size = got.GetUINT64(&MF_MT_FRAME_SIZE)?;
        let (width, height) = ((size >> 32) as u32, (size & 0xffff_ffff) as u32);
        let stride = got.GetUINT32(&MF_MT_DEFAULT_STRIDE).map(|s| s as i32).unwrap_or(width as i32 * 4);
        let duration = reader
            .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
            .ok()
            .and_then(|v| PropVariantToUInt64(&v).ok())
            .map_or(0.0, |d| d as f64 / 1e7);
        if out.send(Msg::Opened { duration }).is_err() {
            return Ok(());
        }
        let mut generation = 0u32;
        let mut looping = true;
        let seek_to = |t: f64| -> windows::core::Result<()> {
            reader.SetCurrentPosition(&GUID::zeroed(), &position(t))
        };
        loop {
            // Orders first: a seek or the loop flag.
            loop {
                match cmds.try_recv() {
                    Ok(Cmd::Seek { t, generation: g }) => {
                        seek_to(t)?;
                        generation = g;
                    }
                    Ok(Cmd::Loop(l)) => looping = l,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ok(()),
                }
            }
            let mut flags = 0u32;
            let mut stamp = 0i64;
            let mut sample: Option<IMFSample> = None;
            reader.ReadSample(VIDEO_STREAM, 0, None, Some(&mut flags), Some(&mut stamp), Some(&mut sample))?;
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                if looping {
                    seek_to(0.0)?;
                    generation += 1;
                    continue;
                }
                if out.send(Msg::End { generation }).is_err() {
                    return Ok(());
                }
                // Wait for an order: a seek, or the game side going away.
                match cmds.recv() {
                    Ok(Cmd::Seek { t, generation: g }) => {
                        seek_to(t)?;
                        generation = g;
                    }
                    Ok(Cmd::Loop(l)) => {
                        looping = l;
                        if l {
                            seek_to(0.0)?;
                            generation += 1;
                        }
                    }
                    Err(_) => return Ok(()),
                }
                continue;
            }
            let Some(sample) = sample else { continue };
            let buffer = sample.ConvertToContiguousBuffer()?;
            let mut data: *mut u8 = std::ptr::null_mut();
            let mut len = 0u32;
            buffer.Lock(&mut data, None, Some(&mut len))?;
            let frame = to_rgba(std::slice::from_raw_parts(data, len as usize), width, height, stride);
            buffer.Unlock()?;
            let Some(frame) = frame else { continue };
            if out.send(Msg::Frame { generation, pts: stamp as f64 / 1e7, frame }).is_err() {
                return Ok(());
            }
        }
    }
}

/// RGB32 rows (blue, green, red, unused; `stride` bytes apart, bottom row
/// first when it is negative) as top-row-first RGBA.
fn to_rgba(src: &[u8], width: u32, height: u32, stride: i32) -> Option<VideoFrame> {
    let (w, h) = (width as usize, height as usize);
    let pitch = stride.unsigned_abs() as usize;
    if pitch < w * 4 || src.len() < pitch * h {
        return None;
    }
    let mut rgba = vec![0u8; w * h * 4];
    for y in 0..h {
        let row = if stride < 0 { h - 1 - y } else { y };
        let s = &src[row * pitch..row * pitch + w * 4];
        let d = &mut rgba[y * w * 4..(y + 1) * w * 4];
        for (px, dst) in s.chunks_exact(4).zip(d.chunks_exact_mut(4)) {
            dst.copy_from_slice(&[px[2], px[1], px[0], 255]);
        }
    }
    Some(VideoFrame { width, height, rgba })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lab_video() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../projects/engine-test-lab/assets/videos/countdown.mp4")
    }

    /// Update until a frame comes (the decoder runs on its own thread), up
    /// to a couple of seconds of wall time.
    fn next_frame(d: &mut MfDecoder, dt: f64, state: PlayState) -> Option<VideoFrame> {
        for _ in 0..400 {
            if let Some(f) = d.update(dt, state) {
                return Some(f);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        None
    }

    fn brightness(f: &VideoFrame) -> f64 {
        f.rgba.chunks(4).map(|p| (p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.0).sum::<f64>() / (f.rgba.len() / 4) as f64
    }

    #[test]
    fn media_foundation_plays_seeks_and_ends() {
        let play = PlayState { playing: true, looping: false, speed: 1.0 };
        let mut d = MfDecoder::open(lab_video());
        let first = next_frame(&mut d, 0.0, play).expect("a first frame");
        assert_eq!((first.width, first.height), (320, 180));
        assert!(brightness(&first) > 20.0, "a picture, not black: {}", brightness(&first));
        // A second of the clock: later frames, and a different picture.
        let mut later = None;
        for _ in 0..30 {
            if let Some(f) = next_frame(&mut d, 1.0 / 30.0, play) {
                later = Some(f);
            }
        }
        let later = later.expect("frames as the clock runs");
        assert_ne!(first.rgba, later.rgba, "the picture moves");
        // Seek near the end and run past it: a video that does not loop ends.
        d.seek(3.8);
        let near_end = next_frame(&mut d, 0.0, play).expect("a frame after the seek");
        assert_eq!((near_end.width, near_end.height), (320, 180));
        for _ in 0..200 {
            d.update(1.0 / 30.0, play);
            if d.ended() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(d.ended(), "it ends");
    }

    #[test]
    fn a_missing_file_fails_quietly() {
        let mut d = MfDecoder::open(PathBuf::from("no/such/video.mp4"));
        // Until the decoding thread has started Media Foundation and found
        // no file - a quarter of a second here, longer on a CI runner, which
        // a fixed 250 ms once failed on - it shows nothing and says nothing.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while !d.failed && std::time::Instant::now() < deadline {
            assert!(d.update(0.1, PlayState { playing: true, looping: true, speed: 1.0 }).is_none());
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(d.failed, "a missing file is reported as failed");
        assert!(d.update(0.1, PlayState { playing: true, looping: true, speed: 1.0 }).is_none(), "and shows nothing");
    }
}
