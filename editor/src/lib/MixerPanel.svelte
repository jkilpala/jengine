<script lang="ts">
  // The Mixer panel: the project's buses as channel strips - volume, mute,
  // where each one feeds, and its effects (EQ, distortion, low-pass,
  // compressor, echo, reverb, in the order sound passes through them). This
  // is `settings.audio_mixer` (Rust `audio::MixerLayout`): the mix a game
  // starts with in every runtime, before any script changes it. While the
  // game plays in the editor every change is heard at once; Save writes it to
  // `project.jengine`, Revert goes back to what is saved.
  import { get } from "svelte/store";
  import { project } from "./stores/projectStore";
  import { isPlaying } from "./stores/playStore";
  import { saveProject } from "./api/project";
  import { logError, logInfo } from "./stores/consoleStore";
  import {
    BUILT_IN,
    EFFECTS,
    addBus,
    depthOf,
    effectOf,
    isUnder,
    parentOf,
    parentsFirst,
    removeBus,
    renameBus,
    shownBuses,
    toLayout,
    withEffect,
    type Bus,
  } from "./mixerLayout";

  let buses = $state<Bus[]>(shownBuses(get(project)?.settings.audio_mixer));
  let savedJson = $state(JSON.stringify(toLayout(shownBuses(get(project)?.settings.audio_mixer))));
  let open = $state<string | null>(null);
  let dirty = $derived(JSON.stringify(toLayout(buses)) !== savedJson);

  // A project opened (or its settings saved elsewhere): start from it.
  $effect(() => {
    const layout = $project?.settings.audio_mixer;
    const json = JSON.stringify(toLayout(shownBuses(layout)));
    if (json !== savedJson) {
      savedJson = json;
      buses = shownBuses(layout);
    }
  });

  /** Hear the mix as it is now, in the running game. */
  async function preview(next: Bus[]) {
    try {
      const wasm = await import("./viewport-wasm/jengine_editor_viewport.js");
      const settings = { ...(get(project)?.settings ?? {}), audio_mixer: toLayout(next) };
      (wasm as unknown as { viewport_set_audio_mixer: (s: string) => void }).viewport_set_audio_mixer(JSON.stringify(settings));
    } catch {
      // The viewport is not up yet; the saved mix reaches it when it is.
    }
  }

  function change(next: Bus[]) {
    buses = parentsFirst(next);
    void preview(buses);
  }

  function setBus(name: string, patch: Partial<Bus>) {
    change(buses.map((b) => (b.name === name ? { ...b, ...patch } : b)));
  }

  async function save() {
    const proj = get(project);
    if (!proj) return;
    const settings = { ...proj.settings, audio_mixer: toLayout($state.snapshot(buses) as Bus[]) };
    try {
      await saveProject(proj.path, proj.name, proj.default_scene ?? null, settings, proj.component_definitions ?? []);
      project.update((p) => (p ? { ...p, settings } : p));
      savedJson = JSON.stringify(settings.audio_mixer);
      logInfo("Saved the project's mixer");
    } catch (e) {
      logError(`Could not save the mixer: ${e}`);
    }
  }

  function revert() {
    const layout = get(project)?.settings.audio_mixer;
    change(shownBuses(layout));
  }

  /** Linear volume as decibels, for the strip's readout. */
  const db = (v: number) => (v <= 0 ? "-inf" : `${(20 * Math.log10(v)).toFixed(1)} dB`);
  const builtIn = (name: string) => (BUILT_IN as readonly string[]).includes(name);
</script>

<div class="mixer">
  <div class="bar">
    <span class="title">Mixer</span>
    <span class="dim">{$isPlaying ? "live in the running game" : "the mix every runtime starts with"}</span>
    <button onclick={() => change(addBus(buses, "master"))}>+ bus</button>
    <button disabled={!dirty} onclick={revert}>Revert</button>
    <button class="save" disabled={!dirty} onclick={save}>Save{dirty ? " *" : ""}</button>
  </div>
  <div class="strips">
    {#each buses as b (b.name)}
      {@const fxCount = Object.keys(b.effects ?? {}).length}
      <div class="strip" class:muted={b.muted} style:--depth={depthOf(buses, b.name)}>
        {#if builtIn(b.name)}
          <div class="name" title="A built-in bus">{b.name}</div>
        {:else}
          <input class="name" value={b.name} title="Rename (scripts and audio sources name it)" onchange={(e) => change(renameBus(buses, b.name, (e.target as HTMLInputElement).value))} />
        {/if}
        {#if b.name !== "master"}
          <select
            title="The bus it feeds"
            value={parentOf(b)}
            onchange={(e) => setBus(b.name, { parent: (e.target as HTMLSelectElement).value })}
          >
            {#each buses.filter((p) => !isUnder(buses, p.name, b.name)) as p}
              <option value={p.name}>→ {p.name}</option>
            {/each}
          </select>
        {:else}
          <div class="dim out">→ output</div>
        {/if}
        <input
          class="fader"
          type="range"
          min="0"
          max="1"
          step="0.01"
          value={b.volume}
          title="Volume (linear)"
          oninput={(e) => setBus(b.name, { volume: parseFloat((e.target as HTMLInputElement).value) })}
        />
        <div class="dim readout">{db(b.volume)}</div>
        <div class="buttons">
          <button class:on={b.muted} title="Mute this bus and everything under it" onclick={() => setBus(b.name, { muted: !b.muted || undefined })}>M</button>
          <button class:on={open === b.name} title="Effects" onclick={() => (open = open === b.name ? null : b.name)}>FX{fxCount ? ` ${fxCount}` : ""}</button>
          {#if !builtIn(b.name)}
            <button title="Remove (its buses move up to its parent)" onclick={() => change(removeBus(buses, b.name))}>×</button>
          {/if}
        </div>
        {#if open === b.name}
          <div class="effects">
            {#each EFFECTS as fx}
              <label title={fx.doc}>
                <span>{fx.label}</span>
                <input
                  type="range"
                  min={fx.min}
                  max={fx.max}
                  step={fx.step}
                  value={effectOf(b, fx.key)}
                  oninput={(e) => change(buses.map((x) => (x.name === b.name ? withEffect(x, fx.key, parseFloat((e.target as HTMLInputElement).value)) : x)))}
                />
                <span class="val">{effectOf(b, fx.key)}</span>
              </label>
            {/each}
          </div>
        {/if}
      </div>
    {/each}
  </div>
</div>

<style>
  .mixer {
    height: 100%;
    display: flex;
    flex-direction: column;
    font-size: 11px;
    color: var(--ed-text);
    background: var(--ed-base);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 3px 8px;
    border-bottom: 1px solid var(--ed-line);
  }
  .title {
    font-weight: 600;
  }
  .dim {
    color: var(--ed-faint);
  }
  .strips {
    flex: 1;
    display: flex;
    gap: 6px;
    padding: 6px;
    overflow: auto;
    align-items: flex-start;
  }
  .strip {
    width: 112px;
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 3px;
    padding: 4px;
    border: 1px solid var(--ed-line);
    border-top: 3px solid hsl(calc(220 - var(--depth) * 40) 60% 65%);
    border-radius: 4px;
  }
  .strip.muted {
    opacity: 0.55;
  }
  .name {
    font-weight: 600;
    width: 100%;
    box-sizing: border-box;
  }
  .out {
    padding: 2px 0;
  }
  .fader {
    width: 100%;
  }
  .readout {
    text-align: center;
    font-family: var(--ed-mono);
  }
  .buttons {
    display: flex;
    gap: 3px;
  }
  .effects {
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .effects label {
    display: grid;
    grid-template-columns: 1fr;
  }
  .effects input {
    width: 100%;
  }
  .val {
    text-align: right;
    font-family: var(--ed-mono);
    color: var(--ed-muted);
  }
  input,
  select,
  button {
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    border-radius: 4px;
    font-size: 11.5px;
    padding: 3px 4px;
  }
  button {
    cursor: pointer;
  }
  button.on {
    border-color: var(--ed-warn);
    color: var(--ed-warn);
  }
  .save:not(:disabled) {
    border-color: var(--ed-accent);
  }
</style>
