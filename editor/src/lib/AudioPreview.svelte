<script lang="ts">
    import { onDestroy } from "svelte";
    import {
        playAudioPreview,
        stopAudioPreview,
        pauseAudioPreview,
        resumeAudioPreview,
        setAudioPreviewVolume,
    } from "./api/audio";

    type PlayState = "stopped" | "playing" | "paused";

    let projectPath = $state("");
    let relativePath = $state("");
    let fileName = $derived(relativePath.split(/[\\/]/).pop() ?? "");
    let visible = $state(false);

    let playState = $state<PlayState>("stopped");
    let volume = $state(80);
    let pitch = $state(1.0);
    let looping = $state(false);

    // Public API: open a file for preview
    export function open(projPath: string, relPath: string) {
        // Stop previous
        if (playState !== "stopped") {
            stopAudioPreview().catch(() => {});
        }

        projectPath = projPath;
        relativePath = relPath;
        visible = true;
        playState = "stopped";

        // Auto-play
        doPlay();
    }

    export function close() {
        if (playState !== "stopped") {
            stopAudioPreview().catch(() => {});
        }
        visible = false;
        playState = "stopped";
        projectPath = "";
        relativePath = "";
    }

    async function doPlay() {
        try {
            await playAudioPreview(projectPath, relativePath, {
                volume: volume / 100,
                pitch,
                looping,
            });
            playState = "playing";
        } catch (e) {
            console.error("Audio playback failed:", e);
            playState = "stopped";
        }
    }

    async function handlePlayPause() {
        if (playState === "stopped") {
            await doPlay();
        } else if (playState === "playing") {
            await pauseAudioPreview();
            playState = "paused";
        } else if (playState === "paused") {
            await resumeAudioPreview();
            playState = "playing";
        }
    }

    async function handleStop() {
        await stopAudioPreview();
        playState = "stopped";
    }

    async function handleRestart() {
        await stopAudioPreview();
        playState = "stopped";
        await doPlay();
    }

    function handleVolumeChange() {
        if (playState !== "stopped") {
            setAudioPreviewVolume(volume / 100).catch(() => {});
        }
    }

    // When looping or pitch change, restart playback to apply
    async function handleSettingChange() {
        if (playState === "playing") {
            await handleRestart();
        }
    }

    onDestroy(() => {
        if (playState !== "stopped") {
            stopAudioPreview().catch(() => {});
        }
    });
</script>

{#if visible}
    <div class="audio-preview">
        <div class="audio-info">
            <span class="audio-icon">🔊</span>
            <span class="audio-name" title={relativePath}>{fileName}</span>
        </div>

        <div class="audio-controls">
            <button
                class="ctrl-btn"
                title={playState === "playing" ? "Pause" : "Play"}
                onclick={handlePlayPause}
                >{playState === "playing" ? "⏸" : "▶"}</button
            >
            <button
                class="ctrl-btn"
                title="Stop"
                onclick={handleStop}
                disabled={playState === "stopped"}>⏹</button
            >
            <button
                class="ctrl-btn"
                title="Restart"
                onclick={handleRestart}
                disabled={playState === "stopped"}>🔄</button
            >
        </div>

        <div class="audio-slider">
            <label>
                🔈
                <input
                    type="range"
                    min="0"
                    max="100"
                    bind:value={volume}
                    oninput={handleVolumeChange}
                />
                <span class="slider-val">{volume}%</span>
            </label>
        </div>

        <div class="audio-slider">
            <label>
                ⏩
                <input
                    type="range"
                    min="0.5"
                    max="2.0"
                    step="0.1"
                    bind:value={pitch}
                    onchange={handleSettingChange}
                />
                <span class="slider-val">{pitch.toFixed(1)}×</span>
            </label>
        </div>

        <label class="loop-toggle">
            <input
                type="checkbox"
                bind:checked={looping}
                onchange={handleSettingChange}
            />
            🔁
        </label>

        <button class="ctrl-btn close-btn" title="Close" onclick={close}
            >✕</button
        >
    </div>
{/if}

<style>
    .audio-preview {
        display: flex;
        align-items: center;
        gap: 10px;
        padding: 6px 12px;
        background: var(--ed-mantle);
        border-top: 1px solid var(--ed-line);
        font-size: 12px;
        color: var(--ed-text);
        min-height: 36px;
    }

    .audio-info {
        display: flex;
        align-items: center;
        gap: 4px;
        min-width: 0;
    }

    .audio-icon {
        font-size: 14px;
    }

    .audio-name {
        font-weight: 600;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
        max-width: 180px;
    }

    .audio-controls {
        display: flex;
        gap: 2px;
    }

    .ctrl-btn {
        padding: 3px 8px;
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        background: var(--ed-base);
        color: var(--ed-text);
        cursor: pointer;
        font-size: 12px;
        line-height: 1;
    }

    .ctrl-btn:hover {
        background: var(--ed-hover);
    }
    .ctrl-btn:disabled {
        opacity: 0.4;
        cursor: default;
    }

    .audio-slider {
        display: flex;
        align-items: center;
    }

    .audio-slider label {
        display: flex;
        align-items: center;
        gap: 4px;
        font-size: 11px;
        color: var(--ed-muted);
    }

    .audio-slider input[type="range"] {
        width: 80px;
        height: 4px;
        accent-color: var(--ed-accent);
    }

    .slider-val {
        font-family: monospace;
        font-size: 10px;
        min-width: 32px;
    }

    .loop-toggle {
        display: flex;
        align-items: center;
        gap: 2px;
        cursor: pointer;
        font-size: 12px;
    }

    .loop-toggle input[type="checkbox"] {
        accent-color: var(--ed-accent);
    }

    .close-btn {
        margin-left: auto;
    }
</style>
