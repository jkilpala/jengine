import { invoke } from "@tauri-apps/api/core";

/**
 * Play an audio file from the project assets.
 * Automatically stops any previously playing preview.
 */
export function playAudioPreview(
  projectPath: string,
  relativePath: string,
  opts: { volume?: number; pitch?: number; looping?: boolean } = {},
): Promise<void> {
  return invoke("play_audio_preview", {
    projectPath,
    relativePath,
    volume: opts.volume ?? 1.0,
    pitch: opts.pitch ?? 1.0,
    looping: opts.looping ?? false,
  });
}

/** Stop the currently playing audio preview. */
export function stopAudioPreview(): Promise<void> {
  return invoke("stop_audio_preview");
}

/** Pause the currently playing audio preview. */
export function pauseAudioPreview(): Promise<void> {
  return invoke("pause_audio_preview");
}

/** Resume the currently paused audio preview. */
export function resumeAudioPreview(): Promise<void> {
  return invoke("resume_audio_preview");
}

/** Update volume of the currently playing sound (0.0–1.0). */
export function setAudioPreviewVolume(volume: number): Promise<void> {
  return invoke("set_audio_preview_volume", { volume });
}

/** Known audio file extensions. */
export const AUDIO_EXTENSIONS = ["ogg", "wav", "mp3", "flac"];

/** Check if a file extension is an audio format. */
export function isAudioFile(ext: string): boolean {
  return AUDIO_EXTENSIONS.includes(ext.toLowerCase());
}
