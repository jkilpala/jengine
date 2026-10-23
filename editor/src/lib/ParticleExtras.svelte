<script lang="ts">
  // The particle emitter fields added after the Inspector's and the particle
  // editor's own rows were written: blend mode, emit shape, stretch, drag,
  // turbulence, trails, flipbooks, lights, the two curves, collision and sub-emitters. One component for both panels, so the
  // next field is added once. `emitter` is read-only here; every edit goes
  // through `onchange(field, value)`, whose caller records the undo step.
  import type {
    EmitShape,
    ParticleBlendMode,
    ParticleCollision,
    ParticleFlipbook,
    ParticleLights,
    ParticleTrail,
    SubEmitter,
    Turbulence,
  } from "./api/particle";
  import CurveEditor from "./CurveEditor.svelte";
  import GradientEditor from "./GradientEditor.svelte";

  let {
    emitter,
    onchange,
  }: {
    emitter: {
      blend_mode?: ParticleBlendMode;
      emit_shape?: EmitShape;
      stretch?: number;
      drag?: number;
      turbulence?: Turbulence | null;
      trail?: ParticleTrail | null;
      flipbook?: ParticleFlipbook | null;
      light?: ParticleLights | null;
      color_curve?: number[][];
      size_curve?: number[][];
      collision?: ParticleCollision | null;
      sub_emitters?: SubEmitter[];
    };
    onchange: (field: string, value: unknown) => void;
  } = $props();

  const BLENDS: ParticleBlendMode[] = ["Alpha", "Additive", "Multiply", "Screen"];
  const SHAPES = ["point", "circle", "rect", "line", "path", "sprite"] as const;

  let shape = $derived(emitter.emit_shape ?? { kind: "point" as const });
  let subError = $state("");

  /** Sub-emitters as JSON text: `[{ on, emitter, count, chance }, ...]`. */
  function setSubEmitters(text: string) {
    const trimmed = text.trim();
    if (trimmed === "") {
      subError = "";
      onchange("sub_emitters", []);
      return;
    }
    try {
      const subs = JSON.parse(trimmed);
      const ok =
        Array.isArray(subs) &&
        subs.every(
          (s) =>
            s &&
            typeof s.emitter === "string" &&
            (s.on === undefined || s.on === "death" || s.on === "collision"),
        );
      if (!ok) throw new Error('each is { "on": "death" | "collision", "emitter": name, "count", "chance" }');
      subError = "";
      onchange(
        "sub_emitters",
        subs.map((s: Partial<SubEmitter>) => ({
          on: s.on ?? "death",
          emitter: s.emitter,
          count: s.count ?? 4,
          chance: s.chance ?? 1,
        })),
      );
    } catch (err) {
      subError = String((err as Error).message ?? err);
    }
  }

  function num(e: Event, fallback = 0): number {
    const v = parseFloat((e.target as HTMLInputElement).value);
    return Number.isFinite(v) ? v : fallback;
  }

  function setShapeKind(kind: string) {
    if (kind === "circle") onchange("emit_shape", { kind, radius: 32, edge: false });
    else if (kind === "rect") onchange("emit_shape", { kind, size: [64, 64] });
    else if (kind === "line") onchange("emit_shape", { kind, length: 128 });
    else if (kind === "path") onchange("emit_shape", { kind });
    else if (kind === "sprite") onchange("emit_shape", { kind, edge: false, cutoff: 0.5 });
    else onchange("emit_shape", { kind: "point" });
  }
</script>

<div class="field-row">
  <label title="How particles combine with what is under them">Blend</label>
  <select
    value={emitter.blend_mode ?? "Alpha"}
    onchange={(e) => onchange("blend_mode", (e.target as HTMLSelectElement).value)}
  >
    {#each BLENDS as b}
      <option value={b}>{b}</option>
    {/each}
  </select>
</div>

<div class="field-row">
  <label title="Where on the emitter a particle starts">Shape</label>
  <select value={shape.kind} onchange={(e) => setShapeKind((e.target as HTMLSelectElement).value)}>
    {#each SHAPES as s}
      <option value={s}>{s}</option>
    {/each}
  </select>
</div>
{#if shape.kind === "circle"}
  <div class="field-row">
    <label>Radius</label>
    <input
      type="number"
      step="1"
      value={shape.radius}
      onchange={(e) => onchange("emit_shape", { ...shape, radius: num(e, 32) })}
    />
    <label class="inline" title="Only the rim: a ring">
      <input
        type="checkbox"
        checked={!!shape.edge}
        onchange={(e) =>
          onchange("emit_shape", { ...shape, edge: (e.target as HTMLInputElement).checked })}
      />
      edge
    </label>
  </div>
{:else if shape.kind === "rect"}
  <div class="field-row">
    <label>Size</label>
    <input
      type="number"
      step="1"
      value={shape.size[0]}
      onchange={(e) => onchange("emit_shape", { ...shape, size: [num(e, 64), shape.size[1]] })}
    />
    <input
      type="number"
      step="1"
      value={shape.size[1]}
      onchange={(e) => onchange("emit_shape", { ...shape, size: [shape.size[0], num(e, 64)] })}
    />
  </div>
{:else if shape.kind === "line"}
  <div class="field-row">
    <label>Length</label>
    <input
      type="number"
      step="1"
      value={shape.length}
      onchange={(e) => onchange("emit_shape", { ...shape, length: num(e, 128) })}
    />
  </div>
{:else if shape.kind === "path"}
  <div class="field-row sub">
    <span class="hint" title="The entity needs a Vector Path; particles start evenly along it">Along this entity's vector path</span>
  </div>
{:else if shape.kind === "sprite"}
  <div class="field-row">
    <label title="Particles start on this entity's sprite's pixels more opaque than this">Cutoff</label>
    <input
      type="number"
      step="0.05"
      min="0"
      max="1"
      value={shape.cutoff ?? 0.5}
      onchange={(e) => onchange("emit_shape", { ...shape, cutoff: Math.min(1, Math.max(0, num(e, 0.5))) })}
    />
    <label class="inline" title="Only the sprite's outline: a burning edge">
      <input
        type="checkbox"
        checked={!!shape.edge}
        onchange={(e) =>
          onchange("emit_shape", { ...shape, edge: (e.target as HTMLInputElement).checked })}
      />
      edge
    </label>
  </div>
{/if}

<div class="field-row">
  <label title="Draw each particle along its flight: its length grows by the distance it covers in this many seconds. Sparks and rain want 0.02 - 0.08">Stretch</label>
  <input
    type="number"
    step="0.01"
    min="0"
    value={emitter.stretch ?? 0}
    onchange={(e) => onchange("stretch", Math.max(0, num(e)))}
  />
</div>
<div class="field-row">
  <label title="Air resistance per second: velocity falls by e^(-drag t) at any frame rate. Firework bursts want 1.5 - 3">Drag</label>
  <input
    type="number"
    step="0.1"
    min="0"
    value={emitter.drag ?? 0}
    onchange={(e) => onchange("drag", Math.max(0, num(e)))}
  />
</div>

<div class="field-row">
  <label title="Carry particles along a swirling flow (the curl of noise): smoke that curls, sparks that eddy">Turbulence</label>
  <input
    type="checkbox"
    checked={!!emitter.turbulence}
    onchange={(e) =>
      onchange(
        "turbulence",
        (e.target as HTMLInputElement).checked
          ? { strength: 60, scale: 120, speed: 0.3 }
          : null,
      )}
  />
</div>
{#if emitter.turbulence}
  {@const t = emitter.turbulence}
  <div class="field-row sub">
    <label title="The flow's speed, world units per second">Strength</label>
    <input
      type="number"
      step="5"
      value={t.strength}
      onchange={(e) => onchange("turbulence", { ...t, strength: num(e, 60) })}
    />
  </div>
  <div class="field-row sub">
    <label title="The size of one swirl, world units">Scale</label>
    <input
      type="number"
      step="5"
      min="1"
      value={t.scale}
      onchange={(e) => onchange("turbulence", { ...t, scale: Math.max(1, num(e, 120)) })}
    />
  </div>
  <div class="field-row sub">
    <label title="How fast the flow changes, swirls per second">Speed</label>
    <input
      type="number"
      step="0.05"
      min="0"
      value={t.speed}
      onchange={(e) => onchange("turbulence", { ...t, speed: Math.max(0, num(e, 0.3)) })}
    />
  </div>
{/if}

<div class="field-row">
  <label title="A ribbon behind each particle through where it has been: streaks, comets, fireworks">Trail</label>
  <input
    type="checkbox"
    checked={!!emitter.trail}
    onchange={(e) =>
      onchange(
        "trail",
        (e.target as HTMLInputElement).checked
          ? { length: 8, interval: 0.03, width: 0.8, taper: true, fade: true }
          : null,
      )}
  />
</div>
{#if emitter.trail}
  {@const tr = emitter.trail}
  <div class="field-row sub">
    <label title="Points kept, 2 to 16 - with Interval, how long the trail lasts">Length</label>
    <input
      type="number"
      step="1"
      min="2"
      max="16"
      value={tr.length}
      onchange={(e) => onchange("trail", { ...tr, length: Math.min(16, Math.max(2, Math.round(num(e, 8)))) })}
    />
  </div>
  <div class="field-row sub">
    <label title="Seconds between two recorded points">Interval</label>
    <input
      type="number"
      step="0.01"
      min="0.005"
      value={tr.interval}
      onchange={(e) => onchange("trail", { ...tr, interval: Math.max(0.005, num(e, 0.03)) })}
    />
  </div>
  <div class="field-row sub">
    <label title="Width at the head, as a share of the particle's size">Width</label>
    <input
      type="number"
      step="0.1"
      min="0"
      value={tr.width}
      onchange={(e) => onchange("trail", { ...tr, width: Math.max(0, num(e, 0.8)) })}
    />
  </div>
  <div class="field-row sub">
    <label title="Narrow to nothing at the tail; fade to nothing at the tail">Taper / fade</label>
    <input type="checkbox" checked={tr.taper} onchange={(e) => onchange("trail", { ...tr, taper: (e.target as HTMLInputElement).checked })} />
    <input type="checkbox" checked={tr.fade} onchange={(e) => onchange("trail", { ...tr, fade: (e.target as HTMLInputElement).checked })} />
  </div>
{/if}

<div class="field-row">
  <label title="Play the texture as a sheet of frames: smoke that billows, flames that flicker, debris of many shapes">Flipbook</label>
  <input
    type="checkbox"
    checked={!!emitter.flipbook}
    onchange={(e) =>
      onchange(
        "flipbook",
        (e.target as HTMLInputElement).checked
          ? { columns: 4, rows: 4, frames: 0, mode: "life", fps: 12, cycles: 1, random_start: false }
          : null,
      )}
  />
</div>
{#if emitter.flipbook}
  {@const fb = emitter.flipbook}
  <div class="field-row sub">
    <label title="Frames across and down the sheet, read left to right, top down">Columns / rows</label>
    <input type="number" step="1" min="1" value={fb.columns}
      onchange={(e) => onchange("flipbook", { ...fb, columns: Math.max(1, Math.round(num(e, 1))) })} />
    <input type="number" step="1" min="1" value={fb.rows}
      onchange={(e) => onchange("flipbook", { ...fb, rows: Math.max(1, Math.round(num(e, 1))) })} />
  </div>
  <div class="field-row sub">
    <label title="How many frames are used - 0 for the whole sheet">Frames</label>
    <input type="number" step="1" min="0" value={fb.frames}
      onchange={(e) => onchange("flipbook", { ...fb, frames: Math.max(0, Math.round(num(e, 0))) })} />
  </div>
  <div class="field-row sub">
    <label title="Over the particle's life, at a frame rate, or one frame picked at birth">Mode</label>
    <select value={fb.mode} onchange={(e) => onchange("flipbook", { ...fb, mode: (e.target as HTMLSelectElement).value })}>
      <option value="life">over its life</option>
      <option value="fps">frames a second</option>
      <option value="random">one random frame</option>
    </select>
  </div>
  {#if fb.mode === "fps"}
    <div class="field-row sub">
      <label title="Frames a second, looping">FPS</label>
      <input type="number" step="1" value={fb.fps} onchange={(e) => onchange("flipbook", { ...fb, fps: Math.max(0, num(e, 12)) })} />
    </div>
  {:else if fb.mode === "life"}
    <div class="field-row sub">
      <label title="Times through the frames over one life">Cycles</label>
      <input type="number" step="0.5" value={fb.cycles} onchange={(e) => onchange("flipbook", { ...fb, cycles: Math.max(0, num(e, 1)) })} />
    </div>
  {/if}
  <div class="field-row sub">
    <label title="Start each particle on a frame of its own, so a crowd does not flicker in step">Random start</label>
    <input type="checkbox" checked={fb.random_start}
      onchange={(e) => onchange("flipbook", { ...fb, random_start: (e.target as HTMLInputElement).checked })} />
  </div>
{/if}

<div class="field-row">
  <label title="Each particle gives off light - a shadowless point light where it is drawn, in its colour: sparks, embers, fireflies">Light</label>
  <input
    type="checkbox"
    checked={!!emitter.light}
    onchange={(e) =>
      onchange(
        "light",
        (e.target as HTMLInputElement).checked
          ? { every: 1, max: 64, radius: 80, intensity: 1, falloff: 2, use_particle_color: true, color: [1, 1, 1], scale_with_size: true, fade: true }
          : null,
      )}
  />
</div>
{#if emitter.light}
  {@const li = emitter.light}
  <div class="field-row sub">
    <label title="One particle in this many carries a light; at most Max an emitter (oldest first)">Every / max</label>
    <input type="number" step="1" min="1" value={li.every}
      onchange={(e) => onchange("light", { ...li, every: Math.max(1, Math.round(num(e, 1))) })} />
    <input type="number" step="1" min="0" value={li.max}
      onchange={(e) => onchange("light", { ...li, max: Math.max(0, Math.round(num(e, 64))) })} />
  </div>
  <div class="field-row sub">
    <label title="The light's reach, and how bright it is">Radius / intensity</label>
    <input type="number" step="5" min="0" value={li.radius}
      onchange={(e) => onchange("light", { ...li, radius: Math.max(0, num(e, 80)) })} />
    <input type="number" step="0.1" min="0" value={li.intensity}
      onchange={(e) => onchange("light", { ...li, intensity: Math.max(0, num(e, 1)) })} />
  </div>
  <div class="field-row sub">
    <label title="Light in the particle's colour; dim with its alpha; shrink with its size">Colour / fade / size</label>
    <input type="checkbox" checked={li.use_particle_color}
      onchange={(e) => onchange("light", { ...li, use_particle_color: (e.target as HTMLInputElement).checked })} />
    <input type="checkbox" checked={li.fade}
      onchange={(e) => onchange("light", { ...li, fade: (e.target as HTMLInputElement).checked })} />
    <input type="checkbox" checked={li.scale_with_size}
      onchange={(e) => onchange("light", { ...li, scale_with_size: (e.target as HTMLInputElement).checked })} />
  </div>
{/if}

<div class="field-row">
  <label title="Hit the scene's colliders (every non-sensor collider on the layers in Mask): rain on a roof, sparks off a wall">Collision</label>
  <input
    type="checkbox"
    checked={!!emitter.collision}
    onchange={(e) =>
      onchange(
        "collision",
        (e.target as HTMLInputElement).checked
          ? { response: "kill", bounce: 0.4, friction: 0.2, mask: 65535 }
          : null,
      )}
  />
</div>
{#if emitter.collision}
  {@const c = emitter.collision}
  <div class="field-row sub">
    <label title="Kill: the particle dies where it hits. Bounce: it comes off the surface">Response</label>
    <select
      value={c.response}
      onchange={(e) => onchange("collision", { ...c, response: (e.target as HTMLSelectElement).value })}
    >
      <option value="kill">Kill</option>
      <option value="bounce">Bounce</option>
    </select>
  </div>
  {#if c.response === "bounce"}
    <div class="field-row sub">
      <label title="The share of its speed into the surface it keeps coming off: 0 stops dead, 1 bounces as hard as it hit">Bounce</label>
      <input
        type="number"
        step="0.05"
        min="0"
        max="1"
        value={c.bounce}
        onchange={(e) => onchange("collision", { ...c, bounce: Math.min(1, Math.max(0, num(e, 0.4))) })}
      />
    </div>
    <div class="field-row sub">
      <label title="The share of its speed along the surface it loses on a bounce">Friction</label>
      <input
        type="number"
        step="0.05"
        min="0"
        max="1"
        value={c.friction}
        onchange={(e) => onchange("collision", { ...c, friction: Math.min(1, Math.max(0, num(e, 0.2))) })}
      />
    </div>
  {/if}
  <div class="field-row sub">
    <label title="The collision layers it hits: a collider counts when its membership shares a bit with this">Mask</label>
    <input
      type="number"
      step="1"
      min="0"
      max="65535"
      value={c.mask}
      onchange={(e) => onchange("collision", { ...c, mask: Math.min(65535, Math.max(0, Math.round(num(e, 65535)))) })}
    />
  </div>
{/if}

<div class="field-col">
  <label title="Other emitters, by entity name, that burst where these particles die or hit something - a splash where rain lands. The target is an ordinary emitter, usually with an emission rate of 0">Sub-emitters</label>
  <textarea
    rows="2"
    spellcheck="false"
    placeholder={'[{"on": "collision", "emitter": "Splash", "count": 3, "chance": 0.5}]'}
    value={emitter.sub_emitters?.length ? JSON.stringify(emitter.sub_emitters) : ""}
    onchange={(e) => setSubEmitters((e.target as HTMLTextAreaElement).value)}
  ></textarea>
  {#if subError}<span class="error">{subError}</span>{/if}
</div>

<div class="field-col">
  <label title="Colour over life: stops over the particle's life, colours linear. Replaces the start/end fade when present; no stops for none">Colour curve</label>
  <GradientEditor keys={emitter.color_curve} onchange={(keys) => onchange("color_curve", keys)} />
</div>
<div class="field-col">
  <label title="Size over life: a multiplier on the start size over the particle's life. Replaces the start/end sizes when present; no keys for none">Size curve</label>
  <CurveEditor keys={emitter.size_curve} fallback={1} onchange={(keys) => onchange("size_curve", keys)} />
</div>

<style>
  .field-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 0;
  }
  .field-row.sub label {
    padding-left: 12px;
  }
  .field-row > label {
    min-width: 90px;
    font-size: 11px;
    color: var(--ed-text);
    flex-shrink: 0;
  }
  .field-row label.inline {
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 3px;
  }
  .field-col {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 2px 0;
  }
  .field-col label {
    font-size: 11px;
    color: var(--ed-text);
  }
  input[type="number"],
  select,
  textarea {
    flex: 1;
    min-width: 0;
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    padding: 2px 4px;
    border-radius: 4px;
    font-size: 12px;
  }
  textarea {
    font-family: var(--ed-mono);
    font-size: 11px;
    resize: vertical;
  }
  .error {
    color: var(--ed-err);
    font-size: 10px;
  }
</style>
