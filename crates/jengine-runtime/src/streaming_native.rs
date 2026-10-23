//! The native runner's half of `jengine_core::scene::streaming`: a chunk or
//! a preload reads and parses its scene on the main thread (cheap), then
//! decodes the textures it needs on a worker thread (the slow part), and is
//! laid into the world - or, for a preload, left in the asset cache - once
//! the last one is in. The frame keeps drawing the whole time.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};

use hecs::World;
use jengine_core::asset::RawTextureData;

/// One load under way.
pub(crate) struct Job {
    pub id: u64,
    /// `Some(handle)` for a chunk, `None` for a preload.
    pub handle: Option<u32>,
    /// The path the script asked for, the preload's key.
    pub requested: String,
    pub offset: [f32; 2],
    pub scene_path: PathBuf,
    /// The chunk's world, waiting to be laid in.
    pub chunk: Option<World>,
    pub waiting: HashSet<String>,
    pub total: usize,
}

impl Job {
    pub fn progress(&self) -> f32 {
        if self.total == 0 {
            return 0.95;
        }
        (self.total - self.waiting.len()) as f32 / self.total as f32 * 0.95
    }
}

/// A texture decoded on the worker: which job, which id, the pixels.
pub(crate) type Decoded = (u64, String, Result<RawTextureData, String>);

pub(crate) struct NativeStreaming {
    pub jobs: Vec<Job>,
    pub next_id: u64,
    pub tx: Sender<Decoded>,
    pub rx: Receiver<Decoded>,
}

impl Default for NativeStreaming {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self { jobs: Vec::new(), next_id: 0, tx, rx }
    }
}

impl NativeStreaming {
    /// Decode `files` (texture id, file) for job `id` on a thread of its own.
    pub fn decode(&self, id: u64, files: Vec<(String, PathBuf)>) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            for (tex, path) in files {
                let raw = RawTextureData::from_file(path.to_string_lossy().as_ref()).map_err(|e| format!("{e:#}"));
                if tx.send((id, tex, raw)).is_err() {
                    return;
                }
            }
        });
    }
}
