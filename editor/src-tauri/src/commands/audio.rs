use std::path::PathBuf;
use std::sync::Mutex;

use jengine_audio::KiraAudioBackend;
use jengine_core::audio::{AudioBackend, ClipHandle, PlaySettings, SoundHandle};
use tauri::State;

/// Managed state for the editor's audio preview.
pub struct AudioPreviewState {
    backend: Option<KiraAudioBackend>,
    /// Currently playing sound handle, if any.
    current_sound: Option<SoundHandle>,
    /// Currently loaded clip path (to avoid reloading).
    current_clip_path: Option<String>,
    /// Handle of the currently loaded clip.
    current_clip: Option<ClipHandle>,
}

impl AudioPreviewState {
    pub fn new() -> Self {
        let backend = KiraAudioBackend::new()
            .map_err(|e| log::error!("Failed to init audio backend: {e}"))
            .ok();
        Self {
            backend,
            current_sound: None,
            current_clip_path: None,
            current_clip: None,
        }
    }
}

/// Play an audio file from the project assets.
#[tauri::command]
pub fn play_audio_preview(
    state: State<'_, Mutex<AudioPreviewState>>,
    project_path: String,
    relative_path: String,
    volume: f32,
    pitch: f32,
    looping: bool,
) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| format!("Lock error: {e}"))?;

    // Stop any currently playing sound first
    let prev_sound = s.current_sound.take();
    if let (Some(backend), Some(handle)) = (s.backend.as_mut(), prev_sound) {
        backend.stop(handle);
    }

    // Load clip if not already loaded or path changed
    let full_id = format!("{}/{}", project_path, relative_path);
    if s.current_clip_path.as_deref() != Some(&full_id) {
        let file_path = PathBuf::from(&project_path)
            .join("assets")
            .join(&relative_path);

        if !file_path.exists() {
            return Err(format!("Audio file not found: {relative_path}"));
        }

        // Prevent path traversal
        let canonical = file_path
            .canonicalize()
            .map_err(|e| format!("Path error: {e}"))?;
        let assets_dir = PathBuf::from(&project_path)
            .join("assets")
            .canonicalize()
            .map_err(|e| format!("Assets dir error: {e}"))?;
        if !canonical.starts_with(&assets_dir) {
            return Err("Access denied: path traversal detected".to_string());
        }

        let data = std::fs::read(&canonical)
            .map_err(|e| format!("Failed to read audio file: {e}"))?;

        let backend = s.backend.as_mut().ok_or("Audio backend not available")?;
        let clip = backend
            .load_clip(&full_id, &data)
            .map_err(|e| format!("Failed to decode audio: {e}"))?;
        s.current_clip = Some(clip);
        s.current_clip_path = Some(full_id);
    }

    let clip = s.current_clip.ok_or("No clip loaded")?;
    let settings = PlaySettings {
        volume: volume.clamp(0.0, 1.0),
        pitch: pitch.clamp(0.1, 4.0),
        looping,
        // Asset preview is centred; panning is a gameplay concern.
        pan: 0.0,
        // Straight to the output: no bus effects on a preview.
        bus: String::new(),
        // Nothing muffles a preview.
        filter: false,
    };

    let backend = s.backend.as_mut().ok_or("Audio backend not available")?;
    let sound = backend
        .play(clip, settings)
        .map_err(|e| format!("Failed to play: {e}"))?;
    s.current_sound = Some(sound);

    Ok(())
}

/// Stop the currently playing audio preview.
#[tauri::command]
pub fn stop_audio_preview(state: State<'_, Mutex<AudioPreviewState>>) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| format!("Lock error: {e}"))?;
    let handle = s.current_sound.take();
    if let (Some(backend), Some(h)) = (s.backend.as_mut(), handle) {
        backend.stop(h);
    }
    Ok(())
}

/// Pause the currently playing audio preview.
#[tauri::command]
pub fn pause_audio_preview(state: State<'_, Mutex<AudioPreviewState>>) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| format!("Lock error: {e}"))?;
    let handle = s.current_sound;
    if let (Some(backend), Some(h)) = (s.backend.as_mut(), handle) {
        backend.pause(h);
    }
    Ok(())
}

/// Resume the currently paused audio preview.
#[tauri::command]
pub fn resume_audio_preview(state: State<'_, Mutex<AudioPreviewState>>) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| format!("Lock error: {e}"))?;
    let handle = s.current_sound;
    if let (Some(backend), Some(h)) = (s.backend.as_mut(), handle) {
        backend.resume(h);
    }
    Ok(())
}

/// Update volume of the currently playing sound.
#[tauri::command]
pub fn set_audio_preview_volume(
    state: State<'_, Mutex<AudioPreviewState>>,
    volume: f32,
) -> Result<(), String> {
    let mut s = state.lock().map_err(|e| format!("Lock error: {e}"))?;
    let handle = s.current_sound;
    if let (Some(backend), Some(h)) = (s.backend.as_mut(), handle) {
        backend.set_volume(h, volume.clamp(0.0, 1.0));
    }
    Ok(())
}
