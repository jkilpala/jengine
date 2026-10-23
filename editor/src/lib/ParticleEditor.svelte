<script lang="ts" module>
    /** Convert [r,g,b,a] (0-1) to #hex for color input. */
    function rgbaToHex(c: [number, number, number, number]): string {
        const r = Math.round(c[0] * 255)
            .toString(16)
            .padStart(2, "0");
        const g = Math.round(c[1] * 255)
            .toString(16)
            .padStart(2, "0");
        const b = Math.round(c[2] * 255)
            .toString(16)
            .padStart(2, "0");
        return `#${r}${g}${b}`;
    }

    /** Convert #hex + alpha to [r,g,b,a] (0-1). */
    function hexToRgba(
        hex: string,
        a: number,
    ): [number, number, number, number] {
        const r = parseInt(hex.slice(1, 3), 16) / 255;
        const g = parseInt(hex.slice(3, 5), 16) / 255;
        const b = parseInt(hex.slice(5, 7), 16) / 255;
        return [r, g, b, a];
    }
</script>

<script lang="ts">
    import { claimUndo } from "./stores/undoTargetStore";
    import { requestEditorClose } from "./stores/editorClose";
    import { onMount, onDestroy } from "svelte";
    import { get } from "svelte/store";
    import {
        particleEmitter,
        particleFilePath,
        particleDirty,
        canUndoParticle,
        canRedoParticle,
        undoParticle,
        redoParticle,
        executeParticle,
        saveCurrentParticle,
        closeParticleEditor,
    } from "./stores/particleEditorStore";
    import { PARTICLE_PRESETS } from "./api/particle-presets";
    import ParticleExtras from "./ParticleExtras.svelte";
    import type { ParticleEmitterData } from "./api/particle";
    import { listProjectImages, readAssetBinary } from "./api/project";
    import { project } from "./stores/projectStore";
    import init, {
        viewport_particle_preview_init,
        viewport_particle_preview_start,
        viewport_particle_preview_update,
        viewport_particle_preview_render,
        viewport_particle_preview_stop,
        viewport_particle_preview_burst,
        viewport_particle_preview_resize,
        viewport_particle_preview_alive_count,
        viewport_particle_preview_set_texture,
        viewport_particle_preview_clear_texture,
    } from "./viewport-wasm/jengine_editor_viewport.js";

    // ---------------------------------------------------------------------------
    // State
    // ---------------------------------------------------------------------------
    let canvasEl: HTMLCanvasElement | undefined = $state();
    let wasmReady = $state(false);
    let playing = $state(true);
    let aliveCount = $state(0);
    let animFrameId = 0;
    let lastTime = 0;
    let resizeObserver: ResizeObserver | undefined;

    // Collapsible sections
    let showEmission = $state(true);
    let showLifetime = $state(true);
    let showMovement = $state(true);
    let showAppearance = $state(true);
    let showShaping = $state(true);
    let showRotation = $state(false);
    let showTexture = $state(false);
    let showAdvanced = $state(false);

    // Texture picker state
    let projectImages = $state<string[]>([]);
    let loadedTextureId = $state<string | null>(null);

    const fileName = $derived(
        $particleFilePath?.split(/[\\/]/).pop()?.replace(".particle", "") ??
            "Particle Emitter",
    );

    // ---------------------------------------------------------------------------
    // WASM preview loop
    // ---------------------------------------------------------------------------
    function startLoop() {
        lastTime = performance.now();
        function frame(now: number) {
            const dt = Math.min((now - lastTime) / 1000, 0.05); // cap at 50ms
            lastTime = now;
            if (playing) {
                viewport_particle_preview_update(dt);
            }
            viewport_particle_preview_render();
            aliveCount = viewport_particle_preview_alive_count();
            animFrameId = requestAnimationFrame(frame);
        }
        animFrameId = requestAnimationFrame(frame);
    }

    function stopLoop() {
        if (animFrameId) {
            cancelAnimationFrame(animFrameId);
            animFrameId = 0;
        }
    }

    // Push current emitter config to the WASM preview
    function syncToPreview() {
        const data = get(particleEmitter);
        if (!data || !wasmReady) return;
        try {
            viewport_particle_preview_start(JSON.stringify(data));
        } catch (e) {
            console.error("[ParticleEditor] preview start failed:", e);
        }
    }

    // ---------------------------------------------------------------------------
    // Lifecycle
    // ---------------------------------------------------------------------------
    onMount(async () => {
        if (!canvasEl) return;
        try {
            await init();
        } catch {
            // Already initialized from Viewport — that's fine
        }

        // Set initial canvas size
        const rect = canvasEl.parentElement!.getBoundingClientRect();
        canvasEl.width = Math.floor(rect.width);
        canvasEl.height = Math.floor(rect.height);

        viewport_particle_preview_init("particle-preview-canvas");
        wasmReady = true;

        syncToPreview();
        startLoop();

        // Resize observer
        resizeObserver = new ResizeObserver((entries) => {
            for (const entry of entries) {
                const w = Math.floor(entry.contentRect.width);
                const h = Math.floor(entry.contentRect.height);
                if (w > 0 && h > 0 && canvasEl) {
                    canvasEl.width = w;
                    canvasEl.height = h;
                    if (wasmReady) viewport_particle_preview_resize(w, h);
                }
            }
        });
        resizeObserver.observe(canvasEl.parentElement!);
    });

    onDestroy(() => {
        stopLoop();
        if (wasmReady) {
            viewport_particle_preview_stop();
        }
        resizeObserver?.disconnect();
    });

    // Re-sync preview whenever emitter data changes
    $effect(() => {
        void $particleEmitter; // track
        if (wasmReady) syncToPreview();
    });

    // Load project images list for texture picker
    $effect(() => {
        const proj = $project;
        if (proj?.path) {
            listProjectImages(proj.path).then((imgs) => {
                projectImages = imgs;
            });
        }
    });

    // Load texture into WASM preview when texture_id changes
    $effect(() => {
        const texId = $particleEmitter?.texture_id ?? null;
        if (!wasmReady) return;
        if (texId === loadedTextureId) return;
        loadedTextureId = texId;
        if (!texId) {
            viewport_particle_preview_clear_texture();
            return;
        }
        const proj = $project;
        if (!proj?.path) return;
        readAssetBinary(proj.path, texId)
            .then((bytes) => {
                try {
                    viewport_particle_preview_set_texture(
                        new Uint8Array(bytes),
                    );
                } catch (e) {
                    console.error("[ParticleEditor] set_texture failed:", e);
                }
            })
            .catch((e) => {
                console.error(
                    `[ParticleEditor] failed to read texture '${texId}':`,
                    e,
                );
                viewport_particle_preview_clear_texture();
            });
    });

    // ---------------------------------------------------------------------------
    // Property helpers
    // ---------------------------------------------------------------------------
    function setField<K extends keyof ParticleEmitterData>(
        field: K,
        value: ParticleEmitterData[K],
    ) {
        executeParticle(`set ${field}`, (d) => {
            (d as any)[field] = value;
        });
    }

    /** Set one element of a tuple field, e.g. lifetime[0]. */
    function setTupleEl(
        field: keyof ParticleEmitterData,
        idx: number,
        value: number,
    ) {
        executeParticle(`set ${field}[${idx}]`, (d) => {
            const arr = (d as any)[field] as number[];
            arr[idx] = value;
        });
    }

    function applyPreset(idx: number) {
        const preset = PARTICLE_PRESETS[idx];
        if (!preset) return;
        executeParticle(`preset: ${preset.label}`, (d) => {
            Object.assign(d, structuredClone(preset.values));
        });
    }

    function handleBurst() {
        const data = get(particleEmitter);
        if (!data || !wasmReady) return;
        const count = data.burst_count > 0 ? data.burst_count : 32;
        viewport_particle_preview_burst(count);
    }

    function togglePlay() {
        playing = !playing;
    }

    function restart() {
        if (wasmReady) syncToPreview();
    }

    // ---------------------------------------------------------------------------
    // Keyboard shortcuts
    // ---------------------------------------------------------------------------
    function handleKeydown(e: KeyboardEvent) {
        if (e.ctrlKey && e.key === "s") {
            e.preventDefault();
            saveCurrentParticle();
        }
    }

    // Ctrl+Z / Ctrl+Y are this editor's while it is open; the app's key
    // handler asks `undoTarget()` before undoing the scene, so one press
    // no longer undoes in both.
    $effect(() => claimUndo({ undo: undoParticle, redo: redoParticle }));
</script>

<svelte:window onkeydown={handleKeydown} />

<div class="particle-editor">
    <!-- Toolbar -->
    <div class="toolbar">
        <span class="toolbar-title">✨ {fileName}</span>
        <div class="toolbar-group">
            <button
                class="tb-btn"
                title="Undo (Ctrl+Z)"
                disabled={!$canUndoParticle}
                onclick={undoParticle}>↩</button
            >
            <button
                class="tb-btn"
                title="Redo (Ctrl+Y)"
                disabled={!$canRedoParticle}
                onclick={redoParticle}>↪</button
            >
        </div>
        <div class="toolbar-group">
            <select
                class="preset-select"
                onchange={(e) => {
                    const idx = parseInt((e.target as HTMLSelectElement).value);
                    if (!isNaN(idx)) applyPreset(idx);
                    (e.target as HTMLSelectElement).value = "";
                }}
            >
                <option value="" disabled selected>Apply preset…</option>
                {#each PARTICLE_PRESETS as preset, i}
                    <option value={i}>{preset.label}</option>
                {/each}
            </select>
        </div>
        <div class="toolbar-spacer"></div>
        <div class="toolbar-group">
            {#if $particleDirty}
                <span class="dirty-dot" title="Unsaved changes">●</span>
            {/if}
            <button
                class="tb-btn"
                title="Save (Ctrl+S)"
                onclick={saveCurrentParticle}>💾</button
            >
            <button class="tb-btn" title="Close" onclick={() => requestEditorClose("particle")}
                >✕</button
            >
        </div>
    </div>

    <div class="body">
        <!-- Properties panel -->
        <div class="props-panel">
            {#if $particleEmitter}
                {@const em = $particleEmitter}

                <!-- Emission -->
                <button
                    class="section-toggle"
                    onclick={() => (showEmission = !showEmission)}
                >
                    <span class="toggle-icon">{showEmission ? "▾" : "▸"}</span>
                    Emission
                </button>
                {#if showEmission}
                    <div class="section">
                        <label class="prop-row">
                            <span class="prop-label">Rate</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={em.emission_rate}
                                min="0"
                                step="1"
                                onchange={(e) =>
                                    setField(
                                        "emission_rate",
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0,
                                    )}
                            />
                        </label>
                        <label class="prop-row">
                            <span class="prop-label">Max particles</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={em.max_particles}
                                min="1"
                                step="1"
                                onchange={(e) =>
                                    setField(
                                        "max_particles",
                                        parseInt(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 256,
                                    )}
                            />
                        </label>
                        <label class="prop-row">
                            <span class="prop-label">Burst count</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={em.burst_count}
                                min="0"
                                step="1"
                                onchange={(e) =>
                                    setField(
                                        "burst_count",
                                        parseInt(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0,
                                    )}
                            />
                        </label>
                        <label class="prop-row">
                            <span class="prop-label">Active</span>
                            <input
                                type="checkbox"
                                checked={em.active}
                                onchange={() => setField("active", !em.active)}
                            />
                        </label>
                    </div>
                {/if}

                <!-- Lifetime -->
                <button
                    class="section-toggle"
                    onclick={() => (showLifetime = !showLifetime)}
                >
                    <span class="toggle-icon">{showLifetime ? "▾" : "▸"}</span>
                    Lifetime
                </button>
                {#if showLifetime}
                    <div class="section">
                        <label class="prop-row">
                            <span class="prop-label">Min</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={em.lifetime[0]}
                                min="0.01"
                                step="0.1"
                                onchange={(e) =>
                                    setTupleEl(
                                        "lifetime",
                                        0,
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0.1,
                                    )}
                            />
                        </label>
                        <label class="prop-row">
                            <span class="prop-label">Max</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={em.lifetime[1]}
                                min="0.01"
                                step="0.1"
                                onchange={(e) =>
                                    setTupleEl(
                                        "lifetime",
                                        1,
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 1.0,
                                    )}
                            />
                        </label>
                    </div>
                {/if}

                <!-- Movement -->
                <button
                    class="section-toggle"
                    onclick={() => (showMovement = !showMovement)}
                >
                    <span class="toggle-icon">{showMovement ? "▾" : "▸"}</span>
                    Movement
                </button>
                {#if showMovement}
                    <div class="section">
                        <label class="prop-row">
                            <span class="prop-label">Speed min</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={em.speed[0]}
                                step="5"
                                onchange={(e) =>
                                    setTupleEl(
                                        "speed",
                                        0,
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0,
                                    )}
                            />
                        </label>
                        <label class="prop-row">
                            <span class="prop-label">Speed max</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={em.speed[1]}
                                step="5"
                                onchange={(e) =>
                                    setTupleEl(
                                        "speed",
                                        1,
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 50,
                                    )}
                            />
                        </label>
                        <label class="prop-row">
                            <span class="prop-label">Angle min (°)</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={Math.round(
                                    (em.angle[0] * 180) / Math.PI,
                                )}
                                step="5"
                                onchange={(e) =>
                                    setTupleEl(
                                        "angle",
                                        0,
                                        ((parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0) *
                                            Math.PI) /
                                            180,
                                    )}
                            />
                        </label>
                        <label class="prop-row">
                            <span class="prop-label">Angle max (°)</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={Math.round(
                                    (em.angle[1] * 180) / Math.PI,
                                )}
                                step="5"
                                onchange={(e) =>
                                    setTupleEl(
                                        "angle",
                                        1,
                                        ((parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0) *
                                            Math.PI) /
                                            180,
                                    )}
                            />
                        </label>
                        <label class="prop-row">
                            <span class="prop-label">Gravity</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={em.gravity_modifier}
                                step="0.1"
                                onchange={(e) =>
                                    setField(
                                        "gravity_modifier",
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0,
                                    )}
                            />
                        </label>
                    </div>
                {/if}

                <!-- Shape, blend, air, curves -->
                <button
                    class="section-toggle"
                    onclick={() => (showShaping = !showShaping)}
                >
                    <span class="toggle-icon">{showShaping ? "▾" : "▸"}</span
                    >
                    Shape, blend &amp; motion
                </button>
                {#if showShaping}
                    <div class="section">
                        <ParticleExtras
                            emitter={em}
                            onchange={(field, value) =>
                                executeParticle(`set ${field}`, (d) => {
                                    (d as any)[field] = value;
                                })}
                        />
                    </div>
                {/if}

                <!-- Appearance -->
                <button
                    class="section-toggle"
                    onclick={() => (showAppearance = !showAppearance)}
                >
                    <span class="toggle-icon">{showAppearance ? "▾" : "▸"}</span
                    >
                    Appearance
                </button>
                {#if showAppearance}
                    <div class="section">
                        <div class="prop-row">
                            <span class="prop-label">Start color</span>
                            <input
                                type="color"
                                class="prop-color"
                                value={rgbaToHex(em.color_start)}
                                onchange={(e) => {
                                    const rgba = hexToRgba(
                                        (e.target as HTMLInputElement).value,
                                        em.color_start[3],
                                    );
                                    setField("color_start", rgba);
                                }}
                            />
                            <input
                                type="number"
                                class="prop-input-sm"
                                title="Alpha"
                                value={Math.round(em.color_start[3] * 100)}
                                min="0"
                                max="100"
                                step="5"
                                onchange={(e) => {
                                    const a =
                                        (parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 100) / 100;
                                    setField("color_start", [
                                        em.color_start[0],
                                        em.color_start[1],
                                        em.color_start[2],
                                        a,
                                    ]);
                                }}
                            />
                        </div>
                        <div class="prop-row">
                            <span class="prop-label">End color</span>
                            <input
                                type="color"
                                class="prop-color"
                                value={rgbaToHex(em.color_end)}
                                onchange={(e) => {
                                    const rgba = hexToRgba(
                                        (e.target as HTMLInputElement).value,
                                        em.color_end[3],
                                    );
                                    setField("color_end", rgba);
                                }}
                            />
                            <input
                                type="number"
                                class="prop-input-sm"
                                title="Alpha"
                                value={Math.round(em.color_end[3] * 100)}
                                min="0"
                                max="100"
                                step="5"
                                onchange={(e) => {
                                    const a =
                                        (parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0) / 100;
                                    setField("color_end", [
                                        em.color_end[0],
                                        em.color_end[1],
                                        em.color_end[2],
                                        a,
                                    ]);
                                }}
                            />
                        </div>
                        <label class="prop-row">
                            <span class="prop-label">Size start min</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={em.size_start[0]}
                                min="0.1"
                                step="1"
                                onchange={(e) =>
                                    setTupleEl(
                                        "size_start",
                                        0,
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 1,
                                    )}
                            />
                        </label>
                        <label class="prop-row">
                            <span class="prop-label">Size start max</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={em.size_start[1]}
                                min="0.1"
                                step="1"
                                onchange={(e) =>
                                    setTupleEl(
                                        "size_start",
                                        1,
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 1,
                                    )}
                            />
                        </label>
                        <label class="prop-row">
                            <span class="prop-label">Size end min</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={em.size_end[0]}
                                min="0"
                                step="1"
                                onchange={(e) =>
                                    setTupleEl(
                                        "size_end",
                                        0,
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0,
                                    )}
                            />
                        </label>
                        <label class="prop-row">
                            <span class="prop-label">Size end max</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={em.size_end[1]}
                                min="0"
                                step="1"
                                onchange={(e) =>
                                    setTupleEl(
                                        "size_end",
                                        1,
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0,
                                    )}
                            />
                        </label>
                    </div>
                {/if}

                <!-- Rotation -->
                <button
                    class="section-toggle"
                    onclick={() => (showRotation = !showRotation)}
                >
                    <span class="toggle-icon">{showRotation ? "▾" : "▸"}</span>
                    Rotation
                </button>
                {#if showRotation}
                    <div class="section">
                        <label class="prop-row">
                            <span class="prop-label">Angular vel min</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={em.angular_velocity[0]}
                                step="0.1"
                                onchange={(e) =>
                                    setTupleEl(
                                        "angular_velocity",
                                        0,
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0,
                                    )}
                            />
                        </label>
                        <label class="prop-row">
                            <span class="prop-label">Angular vel max</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={em.angular_velocity[1]}
                                step="0.1"
                                onchange={(e) =>
                                    setTupleEl(
                                        "angular_velocity",
                                        1,
                                        parseFloat(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0,
                                    )}
                            />
                        </label>
                    </div>
                {/if}

                <!-- Texture -->
                <button
                    class="section-toggle"
                    onclick={() => (showTexture = !showTexture)}
                >
                    <span class="toggle-icon">{showTexture ? "▾" : "▸"}</span>
                    Texture
                </button>
                {#if showTexture}
                    <div class="section">
                        <div class="prop-row">
                            <span class="prop-label">Texture</span>
                            <select
                                class="prop-input"
                                value={em.texture_id ?? ""}
                                onchange={(e) => {
                                    const val = (e.target as HTMLSelectElement)
                                        .value;
                                    setField("texture_id", val || undefined);
                                }}
                            >
                                <option value="">(none — white square)</option>
                                {#each projectImages as img}
                                    <option value={img}>{img}</option>
                                {/each}
                            </select>
                        </div>
                    </div>
                {/if}

                <!-- Advanced -->
                <button
                    class="section-toggle"
                    onclick={() => (showAdvanced = !showAdvanced)}
                >
                    <span class="toggle-icon">{showAdvanced ? "▾" : "▸"}</span>
                    Advanced
                </button>
                {#if showAdvanced}
                    <div class="section">
                        <label class="prop-row">
                            <span class="prop-label">World space</span>
                            <input
                                type="checkbox"
                                checked={em.world_space}
                                onchange={() =>
                                    setField("world_space", !em.world_space)}
                            />
                        </label>
                        <label class="prop-row">
                            <span class="prop-label">Z-order</span>
                            <input
                                type="number"
                                class="prop-input"
                                value={em.z_order}
                                step="1"
                                onchange={(e) =>
                                    setField(
                                        "z_order",
                                        parseInt(
                                            (e.target as HTMLInputElement)
                                                .value,
                                        ) || 0,
                                    )}
                            />
                        </label>
                    </div>
                {/if}
            {/if}
        </div>

        <!-- Preview area -->
        <div class="preview-area">
            <div class="preview-controls">
                <button
                    class="preview-btn"
                    onclick={togglePlay}
                    title={playing ? "Pause" : "Play"}
                >
                    {playing ? "⏸" : "▶"}
                </button>
                <button class="preview-btn" onclick={restart} title="Restart"
                    >🔄</button
                >
                <button class="preview-btn" onclick={handleBurst} title="Burst"
                    >💥</button
                >
                <span class="alive-badge" title="Alive particles"
                    >{aliveCount}</span
                >
            </div>
            <div class="canvas-wrap">
                <canvas id="particle-preview-canvas" bind:this={canvasEl}
                ></canvas>
            </div>
        </div>
    </div>
</div>

<style>
    .particle-editor {
        display: flex;
        flex-direction: column;
        width: 100%;
        height: 100%;
        background: var(--ed-base);
        color: var(--ed-text);
    }

    .toolbar {
        display: flex;
        align-items: center;
        gap: 8px;
        height: 36px;
        padding: 0 8px;
        background: var(--ed-mantle);
        border-bottom: 1px solid var(--ed-line);
        flex-shrink: 0;
    }

    .toolbar-title {
        font-weight: 600;
        font-size: 13px;
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
    }

    .toolbar-group {
        display: flex;
        align-items: center;
        gap: 4px;
    }

    .toolbar-spacer {
        flex: 1;
    }

    .tb-btn {
        background: none;
        border: 1px solid transparent;
        color: var(--ed-text);
        cursor: pointer;
        padding: 3px 6px;
        border-radius: 4px;
        font-size: 14px;
        line-height: 1;
    }
    .tb-btn:hover:not(:disabled) {
        background: var(--ed-line);
    }
    .tb-btn:disabled {
        opacity: 0.35;
        cursor: default;
    }

    .dirty-dot {
        color: var(--ed-warn);
        font-size: 14px;
    }

    .preset-select {
        background: var(--ed-mantle);
        color: var(--ed-text);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        padding: 2px 6px;
        font-size: 12px;
        cursor: pointer;
    }

    .body {
        display: flex;
        flex: 1;
        min-height: 0;
    }

    /* ---- Properties panel ---- */
    .props-panel {
        width: 280px;
        min-width: 200px;
        border-right: 1px solid var(--ed-line);
        overflow-y: auto;
        flex-shrink: 0;
        background: var(--ed-mantle);
    }

    .section-toggle {
        width: 100%;
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 6px 10px;
        background: none;
        border: none;
        border-bottom: 1px solid var(--ed-line);
        color: var(--ed-text);
        font-size: 12px;
        font-weight: 600;
        cursor: pointer;
        text-align: left;
    }
    .section-toggle:hover {
        background: color-mix(in srgb, var(--ed-ink) 4%, transparent);
    }

    .toggle-icon {
        font-size: 10px;
        width: 12px;
        text-align: center;
    }

    .section {
        padding: 6px 10px;
        border-bottom: 1px solid var(--ed-line);
        display: flex;
        flex-direction: column;
        gap: 4px;
    }

    .prop-row {
        display: flex;
        align-items: center;
        gap: 6px;
        font-size: 12px;
    }

    .prop-label {
        width: 100px;
        flex-shrink: 0;
        color: var(--ed-muted);
        font-size: 11px;
    }

    .prop-input {
        flex: 1;
        background: var(--ed-base);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        padding: 2px 6px;
        font-size: 12px;
        min-width: 0;
    }

    .prop-input-sm {
        width: 48px;
        background: var(--ed-base);
        border: 1px solid var(--ed-line);
        border-radius: 4px;
        color: var(--ed-text);
        padding: 2px 4px;
        font-size: 12px;
    }

    .prop-color {
        width: 28px;
        height: 22px;
        border: 1px solid var(--ed-line);
        border-radius: 3px;
        padding: 0;
        cursor: pointer;
        background: none;
    }

    /* ---- Preview area ---- */
    .preview-area {
        flex: 1;
        display: flex;
        flex-direction: column;
        min-width: 0;
        position: relative;
    }

    .preview-controls {
        display: flex;
        align-items: center;
        gap: 6px;
        padding: 4px 8px;
        background: var(--ed-mantle);
        border-bottom: 1px solid var(--ed-line);
        flex-shrink: 0;
    }

    .preview-btn {
        background: none;
        border: 1px solid var(--ed-line);
        color: var(--ed-text);
        cursor: pointer;
        padding: 3px 8px;
        border-radius: 4px;
        font-size: 14px;
    }
    .preview-btn:hover {
        background: var(--ed-line);
    }

    .alive-badge {
        margin-left: auto;
        font-size: 11px;
        color: var(--ed-muted);
        background: color-mix(in srgb, var(--ed-ink) 6%, transparent);
        padding: 1px 8px;
        border-radius: 8px;
    }

    .canvas-wrap {
        flex: 1;
        position: relative;
        min-height: 0;
        background: var(--ed-crust);
    }

    canvas {
        position: absolute;
        inset: 0;
        width: 100%;
        height: 100%;
    }
</style>
