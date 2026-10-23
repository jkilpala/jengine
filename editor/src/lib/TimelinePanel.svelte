<script lang="ts">
  // The dope sheet: the selected entity's `timeline` component, a row per
  // track with its keys on a time ruler, and a row of event keys. Drag a key
  // to retime it, double-click a row to key the track's value there, pick a
  // key to edit its value. Drag the ruler to move the playhead; with Preview
  // on, the scene view shows the scene posed at the playhead (`previewAt`,
  // sampled the engine's way) - shown, never written. *Key* writes every
  // track's field as the scene has it now at the playhead, so a field moved
  // in the Inspector or the scene view becomes a key (record mode). Every
  // change is one undo step on the scene, made through the entity's prefab
  // when it is an instance (`editThroughInstance`).
  import { get } from "svelte/store";
  import { scene } from "./stores/sceneStore";
  import { execute } from "./stores/commandStore";
  import { selectedEntity } from "./stores/selectionStore";
  import { isPlaying } from "./stores/playStore";
  import { scenePreview } from "./stores/viewportStore";
  import { prefabLibrary, scenePrefabs } from "./stores/prefabLibraryStore";
  import { editThroughInstance, viewOf } from "./prefabInstance";
  import type { SceneData } from "./api/scene";
  import {
    EASES,
    addClip,
    addEvent,
    addTrack,
    clipLength,
    currentClipName,
    currentValue,
    editClip,
    freshClipName,
    moveEvent,
    moveKey,
    previewAt,
    removeClip,
    removeEvent,
    removeKey,
    renameClip,
    resolveTarget,
    sampleTrack,
    setKey,
    trackTitle,
    type Clip,
    type Ease,
    type SceneEntity,
    type TimelineData,
  } from "./timelineEdit";

  const ROW = 22;
  const NAME_W = 180;

  let owner = $derived($selectedEntity);
  let entities = $derived(($scene.entities ?? []).map((e) => viewOf(e, $scenePrefabs)) as SceneEntity[]);
  let timeline = $derived(
    owner >= 0 ? ((entities[owner]?.components?.timeline as TimelineData | undefined) ?? null) : null,
  );
  let chosen = $state<string | null>(null);
  let clipName = $derived(
    timeline ? (chosen && chosen in (timeline.clips ?? {}) ? chosen : currentClipName(timeline)) : null,
  );
  let clip: Clip | null = $derived(timeline && clipName ? (timeline.clips?.[clipName] ?? null) : null);
  let length = $derived(clip ? Math.max(clipLength(clip), 1) : 1);

  let playhead = $state(0);
  let preview = $state(false);
  let zoom = $state(120); // pixels a second
  let selKey = $state<{ track: number; key: number } | null>(null);
  let selEvent = $state<number | null>(null);
  let valueError = $state("");
  let previewPlaying = $state(false);

  // Adding a track.
  let newTarget = $state("");
  let newComponent = $state("transform");
  let newField = $state("position");

  // ── Editing ──

  /** Change the timeline through the scene's history (one undo step). */
  function editTimeline(label: string, fn: (tl: TimelineData) => TimelineData) {
    const idx = owner;
    const prefabs = $scenePrefabs;
    execute(label, (s) => {
      const ent = s.entities[idx];
      if (!ent) return s;
      editThroughInstance(ent, prefabs, (view) => {
        const comps = view.components as Record<string, unknown>;
        comps.timeline = fn((comps.timeline ?? {}) as TimelineData);
      });
      return s;
    });
  }

  function onClip(label: string, fn: (clip: Clip) => void) {
    const name = clipName;
    if (!name) return;
    editTimeline(label, (tl) => editClip(tl, name, fn));
  }

  function newClip() {
    const name = freshClipName(timeline ?? {});
    editTimeline("Add timeline clip", (tl) => addClip(tl, name));
    chosen = name;
  }

  function renameCurrent(to: string) {
    const from = clipName;
    if (!from) return;
    editTimeline("Rename timeline clip", (tl) => renameClip(tl, from, to));
    chosen = to.trim() || from;
  }

  function deleteCurrent() {
    const name = clipName;
    if (!name) return;
    editTimeline("Remove timeline clip", (tl) => removeClip(tl, name));
    chosen = null;
  }

  // ── The playhead and the preview ──

  const timeAt = (x: number) => Math.max(0, x / zoom);
  const xAt = (t: number) => t * zoom;

  $effect(() => {
    // Re-pose whenever the scene, the clip or the playhead changes; stop
    // posing when Preview is off, the selection has no clip, or the game runs.
    if (preview && clip && owner >= 0 && !$isPlaying) {
      const posed = previewAt(entities, owner, clip, playhead);
      scenePreview.set({ ...(get(scene) as SceneData), entities: posed as SceneData["entities"] });
    } else {
      scenePreview.set(null);
    }
  });
  $effect(() => () => scenePreview.set(null));

  function scrub(e: PointerEvent) {
    const el = e.currentTarget as HTMLElement;
    const r = el.getBoundingClientRect();
    const set = (ev: PointerEvent) => (playhead = Math.round(timeAt(ev.clientX - r.left + el.scrollLeft) * 100) / 100);
    set(e);
    preview = true;
    const move = (ev: PointerEvent) => set(ev);
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  function togglePlay() {
    if (previewPlaying) {
      previewPlaying = false;
      return;
    }
    preview = true;
    previewPlaying = true;
    let last = performance.now();
    const tick = (now: number) => {
      if (!previewPlaying) return;
      playhead += (now - last) / 1000;
      last = now;
      if (playhead > length) playhead = timeline?.looping ? 0 : length;
      if (playhead >= length && !timeline?.looping) previewPlaying = false;
      requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  }

  // ── Keys ──

  function keyDrag(e: PointerEvent, track: number, key: number) {
    e.stopPropagation();
    selKey = { track, key };
    selEvent = null;
    const startX = e.clientX;
    const startT = clip?.tracks?.[track]?.keys?.[key]?.[0] ?? 0;
    let t = startT;
    const target = e.currentTarget as HTMLElement;
    const move = (ev: PointerEvent) => {
      t = Math.max(0, Math.round((startT + (ev.clientX - startX) / zoom) * 100) / 100);
      target.style.left = `${xAt(t) - 5}px`;
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      if (t !== startT) {
        let moved = key;
        onClip("Move timeline key", (c) => (moved = moveKey(c, track, key, t)));
        selKey = { track, key: moved };
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  /** Key track `i` where the row was double-clicked, at the value it has
   *  there now - so a key added mid-way does not change the motion. */
  function keyHere(e: MouseEvent, i: number) {
    const el = e.currentTarget as HTMLElement;
    const t = Math.round(timeAt(e.clientX - el.getBoundingClientRect().left) * 100) / 100;
    const track = clip?.tracks?.[i];
    if (!track) return;
    const value = sampleTrack(track, t) ?? valueNow(i);
    let at = -1;
    onClip("Add timeline key", (c) => (at = setKey(c, i, t, value)));
    selKey = { track: i, key: at };
  }

  /** A track's field as the scene has it now. */
  function valueNow(i: number): unknown {
    const track = clip?.tracks?.[i];
    if (!track?.component || !track.field) return undefined;
    const target = resolveTarget(entities, owner, track.target);
    return currentValue(entities[target], track.component, track.field);
  }

  /** Record: every track's field as the scene has it now, keyed at the
   *  playhead. */
  function keyAll() {
    const values = (clip?.tracks ?? []).map((_, i) => valueNow(i));
    onClip("Key timeline at playhead", (c) => {
      values.forEach((v, i) => {
        if (v !== undefined) setKey(c, i, playhead, v);
      });
    });
  }

  function setSelectedValue(text: string) {
    const sel = selKey;
    const key = sel ? clip?.tracks?.[sel.track]?.keys?.[sel.key] : null;
    if (!sel || !key) return;
    let value: unknown;
    try {
      value = JSON.parse(text);
    } catch {
      if (/^[\w./@:-]+$/.test(text.trim())) value = text.trim();
      else {
        valueError = "not JSON";
        return;
      }
    }
    valueError = "";
    onClip("Set timeline key", (c) => setKey(c, sel.track, key[0], value));
  }

  function deleteSelected() {
    if (selKey) {
      const { track, key } = selKey;
      onClip("Remove timeline key", (c) => removeKey(c, track, key));
      selKey = null;
    } else if (selEvent !== null) {
      const e = selEvent;
      onClip("Remove timeline event", (c) => removeEvent(c, e));
      selEvent = null;
    }
  }

  // ── Events ──

  function eventDrag(e: PointerEvent, ev: number) {
    e.stopPropagation();
    selEvent = ev;
    selKey = null;
    const startX = e.clientX;
    const startT = clip?.events?.[ev]?.[0] ?? 0;
    let t = startT;
    const el = e.currentTarget as HTMLElement;
    const move = (pe: PointerEvent) => {
      t = Math.max(0, Math.round((startT + (pe.clientX - startX) / zoom) * 100) / 100);
      el.style.left = `${xAt(t) - 5}px`;
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      if (t !== startT) {
        let moved = ev;
        onClip("Move timeline event", (c) => (moved = moveEvent(c, ev, t)));
        selEvent = moved;
      }
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  function eventHere(e: MouseEvent) {
    const el = e.currentTarget as HTMLElement;
    const t = Math.round(timeAt(e.clientX - el.getBoundingClientRect().left) * 100) / 100;
    let at = -1;
    onClip("Add timeline event", (c) => (at = addEvent(c, t, "event")));
    selEvent = at;
    selKey = null;
  }

  function renameSelectedEvent(name: string) {
    const ev = selEvent;
    if (ev === null || !name.trim()) return;
    onClip("Rename timeline event", (c) => {
      if (c.events?.[ev]) c.events[ev][1] = name.trim();
    });
  }

  // ── Tracks ──

  function addNewTrack() {
    if (!newComponent.trim() || !newField.trim()) return;
    const track = { target: newTarget, component: newComponent.trim(), field: newField.trim() };
    const t = resolveTarget(entities, owner, newTarget);
    const now = currentValue(entities[t], track.component, track.field);
    onClip("Add timeline track", (c) => {
      addTrack(c, track);
      if (now !== undefined) setKey(c, (c.tracks?.length ?? 1) - 1, playhead, now);
    });
  }

  let targetChoices = $derived(entities.map((e) => e.name).filter((n, i, a) => n && a.indexOf(n) === i));
  let componentChoices = $derived.by(() => {
    const t = resolveTarget(entities, owner, newTarget);
    return Object.keys(entities[t]?.components ?? {}).filter((k) => k !== "timeline" && k !== "prefab_instance");
  });

  let ticks = $derived.by(() => {
    const step = zoom >= 200 ? 0.25 : zoom >= 80 ? 0.5 : 1;
    const out: number[] = [];
    for (let t = 0; t <= length + 1e-6; t += step) out.push(Math.round(t * 100) / 100);
    return out;
  });
  let selectedKeyValue = $derived.by(() => {
    if (!selKey) return null;
    const k = clip?.tracks?.[selKey.track]?.keys?.[selKey.key];
    return k ? JSON.stringify(k[1]) : null;
  });
</script>

<div class="timeline-panel">
  {#if owner < 0 || !entities[owner]}
    <div class="empty">Select an entity with a <code>timeline</code> component.</div>
  {:else if !timeline}
    <div class="empty">
      <b>{entities[owner].name}</b> has no <code>timeline</code> component - add one in the Inspector.
    </div>
  {:else}
    <div class="bar">
      <span class="who">{entities[owner].name}</span>
      <select value={clipName ?? ""} onchange={(e) => (chosen = (e.target as HTMLSelectElement).value)}>
        {#each Object.keys(timeline.clips ?? {}).sort() as name}
          <option value={name}>{name}</option>
        {/each}
      </select>
      <button title="Add a clip" onclick={newClip}>+ clip</button>
      {#if clipName}
        <input
          class="name"
          value={clipName}
          title="Rename the clip"
          onchange={(e) => renameCurrent((e.target as HTMLInputElement).value)}
        />
        <button title="Remove the clip" onclick={deleteCurrent}>×</button>
        <label title="The clip's length in seconds; 0 takes its last key">
          len
          <input
            class="num"
            type="number"
            step="0.1"
            min="0"
            value={clip?.duration ?? 0}
            onchange={(e) => {
              const v = Math.max(0, parseFloat((e.target as HTMLInputElement).value) || 0);
              onClip("Set clip length", (c) => (c.duration = v));
            }}
          />
        </label>
        <span class="sep"></span>
        <button class:on={previewPlaying} title="Play the clip in the scene view (preview only)" onclick={togglePlay}
          >{previewPlaying ? "■" : "▶"}</button
        >
        <label title="Show the scene posed at the playhead (nothing is written)">
          <input type="checkbox" bind:checked={preview} /> Preview
        </label>
        <span class="time">{playhead.toFixed(2)}s</span>
        <button title="Key every track's field as the scene has it now, at the playhead" onclick={keyAll}>◆ Key</button>
        <label title="Zoom">
          <input type="range" min="40" max="400" bind:value={zoom} />
        </label>
      {/if}
    </div>

    {#if clip}
      <div class="sheet">
        <div class="names">
          <div class="ruler-name"></div>
          <div class="row-name events-name" title="Event keys call on_<name> on the timeline's script">events</div>
          {#each clip.tracks ?? [] as track, i}
            <div class="row-name" title={trackTitle(track)}>
              <span class="track-title">{trackTitle(track)}</span>
              <select
                value={track.ease ?? "linear"}
                title="How the track moves between keys"
                onchange={(e) => onClip("Set track ease", (c) => {
                  if (c.tracks?.[i]) c.tracks[i].ease = (e.target as HTMLSelectElement).value as Ease;
                })}
              >
                {#each EASES as ease}<option value={ease}>{ease}</option>{/each}
              </select>
              <button
                title="Remove the track"
                onclick={() => onClip("Remove timeline track", (c) => (c.tracks = (c.tracks ?? []).filter((_, j) => j !== i)))}
                >×</button
              >
            </div>
          {/each}
        </div>
        <div class="lanes" style:--w="{xAt(length) + 40}px">
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="ruler" onpointerdown={scrub}>
            {#each ticks as t}
              <span class="tick" style:left="{xAt(t)}px">{t % 1 === 0 ? t : ""}</span>
            {/each}
          </div>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="lane" ondblclick={eventHere} title="Double-click to add an event key">
            {#each clip.events ?? [] as ev, j}
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div
                class="key event"
                class:selected={selEvent === j}
                style:left="{xAt(ev[0]) - 5}px"
                title="{ev[0]}s: on_{ev[1]}"
                onpointerdown={(e) => eventDrag(e, j)}
              ></div>
              <span class="event-label" style:left="{xAt(ev[0]) + 7}px">{ev[1]}</span>
            {/each}
          </div>
          {#each clip.tracks ?? [] as track, i}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div class="lane" ondblclick={(e) => keyHere(e, i)} title="Double-click to key the track here">
              {#each track.keys ?? [] as key, k}
                <!-- svelte-ignore a11y_no_static_element_interactions -->
                <div
                  class="key"
                  class:selected={selKey?.track === i && selKey?.key === k}
                  style:left="{xAt(key[0]) - 5}px"
                  title="{key[0]}s: {JSON.stringify(key[1])}"
                  onpointerdown={(e) => keyDrag(e, i, k)}
                ></div>
              {/each}
            </div>
          {/each}
          <div class="playhead" style:left="{xAt(playhead)}px" style:height="{ROW * ((clip.tracks?.length ?? 0) + 2)}px"></div>
        </div>
      </div>

      <div class="bar">
        {#if selKey && selectedKeyValue !== null}
          <span class="dim">key at {clip.tracks?.[selKey.track]?.keys?.[selKey.key]?.[0]}s</span>
          <input
            class="value"
            value={selectedKeyValue}
            title="The key's value as JSON (a bare word is a string)"
            onchange={(e) => setSelectedValue((e.target as HTMLInputElement).value)}
          />
          {#if valueError}<span class="error">{valueError}</span>{/if}
          <button onclick={deleteSelected}>Delete key</button>
        {:else if selEvent !== null && clip.events?.[selEvent]}
          <span class="dim">event at {clip.events[selEvent][0]}s - calls on_</span>
          <input class="name" value={clip.events[selEvent][1]} onchange={(e) => renameSelectedEvent((e.target as HTMLInputElement).value)} />
          <button onclick={deleteSelected}>Delete event</button>
        {:else}
          <span class="dim">Add a track:</span>
          <select bind:value={newTarget} title="Which entity: empty is the timeline's own">
            <option value="">(this entity)</option>
            {#each targetChoices as n}<option value={n}>{n}</option>{/each}
          </select>
          <input class="name" list="tl-components" bind:value={newComponent} placeholder="component" />
          <datalist id="tl-components">
            {#each componentChoices as c}<option value={c}></option>{/each}
          </datalist>
          <input class="name" bind:value={newField} placeholder="field (position, color.3)" />
          <button onclick={addNewTrack}>+ track</button>
        {/if}
      </div>
    {:else}
      <div class="empty">No clips yet - <button onclick={newClip}>+ clip</button></div>
    {/if}
  {/if}
</div>

<style>
  .timeline-panel {
    display: flex;
    flex-direction: column;
    height: 100%;
    font-size: 11px;
    color: var(--ed-text);
    background: var(--ed-base);
    overflow: hidden;
  }
  .empty {
    padding: 10px;
    color: var(--ed-muted);
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 3px 6px;
    border-bottom: 1px solid var(--ed-line);
    flex-wrap: wrap;
  }
  .who {
    font-weight: 600;
    color: var(--ed-accent);
  }
  .sep {
    width: 8px;
  }
  .time {
    font-family: var(--ed-mono);
    min-width: 44px;
  }
  .dim {
    color: var(--ed-muted);
  }
  .error {
    color: var(--ed-err);
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
    border-color: var(--ed-ok);
  }
  .num {
    width: 48px;
  }
  .name {
    width: 110px;
  }
  .value {
    flex: 1;
    min-width: 120px;
    font-family: var(--ed-mono);
  }
  .sheet {
    display: flex;
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .names {
    width: 180px;
    flex-shrink: 0;
    border-right: 1px solid var(--ed-line);
  }
  .ruler-name,
  .row-name {
    height: 22px;
    display: flex;
    align-items: center;
    gap: 3px;
    padding: 0 4px;
    border-bottom: 1px solid var(--ed-hover);
  }
  .events-name {
    color: var(--ed-warn);
  }
  .track-title {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .lanes {
    position: relative;
    width: var(--w);
    min-width: 100%;
  }
  .ruler {
    position: relative;
    height: 22px;
    border-bottom: 1px solid var(--ed-line);
    cursor: ew-resize;
    background: var(--ed-mantle);
  }
  .tick {
    position: absolute;
    top: 0;
    height: 100%;
    border-left: 1px solid var(--ed-line-strong);
    padding-left: 2px;
    color: var(--ed-faint);
    font-size: 9px;
  }
  .lane {
    position: relative;
    height: 22px;
    border-bottom: 1px solid var(--ed-hover);
  }
  .key {
    position: absolute;
    top: 6px;
    width: 10px;
    height: 10px;
    transform: rotate(45deg);
    background: var(--ed-accent);
    border: 1px solid var(--ed-base);
    cursor: ew-resize;
  }
  .key.event {
    background: var(--ed-warn);
  }
  .key.selected {
    background: var(--ed-accent-high);
    outline: 1px solid var(--ed-ink);
  }
  .event-label {
    position: absolute;
    top: 4px;
    color: var(--ed-warn);
    font-size: 10px;
    pointer-events: none;
  }
  .playhead {
    position: absolute;
    top: 0;
    width: 1px;
    background: var(--ed-err);
    pointer-events: none;
  }
</style>
