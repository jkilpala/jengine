<script lang="ts">
  import { themeColors } from "./stores/themeColors";
  // The skeletal editor, opened on a `.skelanim` clip or a `.rig`.
  //
  // A clip: a preview of a skeleton posed at the playhead - one of the
  // project's rigs or a skeleton in the open scene, the one naming most of
  // the clip's bones picked first - and a dope sheet below it, a row per
  // bone (its keys at each time) and, for the selected bone, a row per
  // channel. Drag a bone in the preview to turn it (Shift to move its
  // joint) and that is keyed at the playhead; drag keys to move them,
  // double-click a channel row to key it, Delete removes, K keys the
  // selected bone's pose, Space plays.
  //
  // A rig: the same preview over its bind pose, and the bones as a tree -
  // name, parent, position, rotation, scale, length - with bones added
  // at their parent's tip and removed without moving their children.
  //
  // Sampling and posing are `skelEdit.ts`, checked against the engine;
  // undo is the settle-based history.
  import { readTextFile, writeTextFile } from "@tauri-apps/plugin-fs";
  import { openInCodeEditor } from "./api/project";
  import { project, requestAssetRefresh } from "./stores/projectStore";
  import { projectAssets } from "./stores/projectAssetsStore";
  import { scene } from "./stores/sceneStore";
  import { assetsWithExtensions } from "./fieldValues";
  import { logError, logInfo } from "./stores/consoleStore";
  import { SettledHistory } from "./settledHistory";
  import { claimUndo } from "./stores/undoTargetStore";
  import {
    CHANNELS,
    addBone,
    boneAt,
    bounds,
    clipId,
    coverage,
    formatClip,
    formatRig,
    keyPose,
    keyTimes,
    moveBoneKeys,
    moveKey,
    parseClip,
    parseRig,
    poseAt,
    positionAt,
    removeBone,
    removeBoneKeys,
    removeKey,
    removeTrack,
    renameBone,
    rotationToward,
    sceneSkeletons,
    setBone,
    setKey,
    skeletonFromRig,
    skeletonFromScene,
    snap,
    trackFor,
    valueAt,
    worldPose,
    type Channel,
    type Rig,
    type SkelBone,
    type SkelClip,
  } from "./skelEdit";

  let { projectPath, relativePath, onclose }: { projectPath: string; relativePath: string; onclose: () => void } =
    $props();

  const isRig = $derived(relativePath.toLowerCase().endsWith(".rig"));

  interface Doc {
    clip: SkelClip | null;
    rig: Rig | null;
  }
  let doc = $state<Doc>({ clip: null, rig: null });
  let dirty = $state(false);
  let error = $state("");

  const history = new SettledHistory<Doc>(
    () => $state.snapshot(doc) as Doc,
    (v) => {
      doc = v;
      dirty = true;
    },
  );
  $effect(() => {
    JSON.stringify(doc);
    history.observe();
  });
  $effect(() => claimUndo({ undo: () => history.undo(), redo: () => history.redo() }));

  $effect(() => {
    void load(relativePath);
  });

  async function load(path: string) {
    try {
      const text = await readTextFile(`${projectPath}/${path}`);
      doc = path.toLowerCase().endsWith(".rig") ? { clip: null, rig: parseRig(text) } : { clip: parseClip(text), rig: null };
      dirty = false;
      history.reset();
    } catch (e) {
      error = `Could not read ${path}: ${e}`;
    }
  }

  function edit(next: Partial<Doc>) {
    doc = { ...doc, ...next };
    dirty = true;
  }

  // ── Skeletons to preview a clip on ──

  interface Source {
    label: string;
    skel: SkelBone[];
  }
  let rigSources = $state<Source[]>([]);
  let rigFiles = $derived(assetsWithExtensions($projectAssets, ["rig"]));
  $effect(() => {
    if (isRig) return;
    const files = rigFiles;
    Promise.all(
      files.map(async (f) => {
        try {
          const rig = parseRig(await readTextFile(`${projectPath}/assets/${f}`));
          return { label: `rig: ${f}`, skel: skeletonFromRig(rig) };
        } catch {
          return null;
        }
      }),
    ).then((list) => (rigSources = list.filter((s): s is Source => s !== null)));
  });
  let sceneSources = $derived(
    sceneSkeletons(($scene?.entities ?? []) as any).map((s) => ({
      label: `scene: ${s.name}`,
      skel: skeletonFromScene(($scene?.entities ?? []) as any, s.index),
    })),
  );
  let sources = $derived<Source[]>(
    isRig ? (doc.rig ? [{ label: "this rig", skel: skeletonFromRig(doc.rig) }] : []) : [...sceneSources, ...rigSources],
  );
  let chosenLabel = $state<string | null>(null);
  let trackBones = $derived((doc.clip?.tracks ?? []).map((t) => t.bone_name));
  // The chosen skeleton, or the one naming most of the clip's bones.
  let source = $derived.by(() => {
    const picked = sources.find((s) => s.label === chosenLabel);
    if (picked) return picked;
    let best: Source | undefined;
    let bestN = -1;
    for (const s of sources) {
      const n = coverage(s.skel, trackBones);
      if (n > bestN) {
        best = s;
        bestN = n;
      }
    }
    return best;
  });
  let skel = $derived<SkelBone[]>(source?.skel ?? []);

  // ── Time ──

  let t = $state(0);
  let snapStep = $state(0.05);
  let playing = $state(false);
  let duration = $derived(Math.max(doc.clip?.duration ?? 1, 0.01));

  $effect(() => {
    if (!playing) return;
    let last = performance.now();
    let raf = 0;
    const step = (now: number) => {
      t = (t + (now - last) / 1000) % duration;
      last = now;
      raf = requestAnimationFrame(step);
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
  });

  let posed = $derived(poseAt(skel, doc.clip, t));
  let world = $derived(worldPose(posed));

  // ── Selection ──

  let selBone = $state<string | null>(null);
  /** A selected key: a channel's key, or (channel null) a bone's keys at
   *  one time. */
  let selKey = $state<{ bone: string; channel: Channel | null; k: number; time: number } | null>(null);

  // Every bone the preview or the clip names, skeleton order first.
  let rows = $derived.by(() => {
    const out = skel.map((b) => b.name);
    for (const n of trackBones) if (!out.includes(n)) out.push(n);
    return out;
  });

  // ── The preview ──

  let canvas = $state<HTMLCanvasElement | null>(null);
  let cw = $state(400);
  let ch = $state(300);
  let view = $state<{ cx: number; cy: number; scale: number } | null>(null);

  // Frame every pose the clip passes through, not only its first.
  function frame() {
    const b = bounds(
      Array.from({ length: doc.clip ? 13 : 1 }, (_, i) => worldPose(poseAt(skel, doc.clip, (i / 12) * duration))).flat(),
    );
    const w = Math.max(b[2] - b[0], 20);
    const h = Math.max(b[3] - b[1], 20);
    view = { cx: (b[0] + b[2]) / 2, cy: (b[1] + b[3]) / 2, scale: Math.min(cw / (w * 1.4), ch / (h * 1.4)) };
  }
  // Frame a new skeleton.
  let framedFor = "";
  $effect(() => {
    const key = `${source?.label}:${skel.length}:${cw}x${ch}`;
    if (key !== framedFor && skel.length > 0) {
      framedFor = key;
      frame();
    }
  });

  const toScreen = (p: [number, number]): [number, number] => {
    const v = view ?? { cx: 0, cy: 0, scale: 1 };
    return [cw / 2 + (p[0] - v.cx) * v.scale, ch / 2 - (p[1] - v.cy) * v.scale];
  };
  const toWorld = (x: number, y: number): [number, number] => {
    const v = view ?? { cx: 0, cy: 0, scale: 1 };
    return [(x - cw / 2) / v.scale + v.cx, -(y - ch / 2) / v.scale + v.cy];
  };

  $effect(() => {
    const el = canvas;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      cw = el.clientWidth;
      ch = el.clientHeight;
    });
    ro.observe(el);
    return () => ro.disconnect();
  });

  let hoverBone = $state(-1);

  $effect(() => {
    const el = canvas;
    if (!el) return;
    const dpr = window.devicePixelRatio || 1;
    el.width = Math.round(cw * dpr);
    el.height = Math.round(ch * dpr);
    const g = el.getContext("2d");
    if (!g) return;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.fillStyle = $themeColors.mantle;
    g.fillRect(0, 0, cw, ch);
    // The skeleton's origin.
    const [ox, oy] = toScreen([0, 0]);
    g.strokeStyle = $themeColors.line;
    g.lineWidth = 1;
    g.beginPath();
    g.moveTo(0, oy);
    g.lineTo(cw, oy);
    g.moveTo(ox, 0);
    g.lineTo(ox, ch);
    g.stroke();
    const animated = new Set(trackBones);
    world.forEach((w, i) => {
      const b = posed[i];
      const [ax, ay] = toScreen(w.origin);
      const [bx, by] = toScreen(w.tip);
      const len = Math.hypot(bx - ax, by - ay);
      const sel = b.name === selBone;
      const color = sel ? $themeColors.warn : i === hoverBone ? $themeColors.text : animated.has(b.name) ? $themeColors.accent : $themeColors.faint;
      if (len > 1) {
        // A kite from joint to tip.
        const ux = (bx - ax) / len;
        const uy = (by - ay) / len;
        const wdt = Math.min(Math.max(len * 0.12, 3), 10);
        const mx = ax + ux * Math.min(len * 0.2, 14);
        const my = ay + uy * Math.min(len * 0.2, 14);
        g.beginPath();
        g.moveTo(ax, ay);
        g.lineTo(mx - uy * wdt, my + ux * wdt);
        g.lineTo(bx, by);
        g.lineTo(mx + uy * wdt, my - ux * wdt);
        g.closePath();
        g.fillStyle = color + "55";
        g.fill();
        g.strokeStyle = color;
        g.lineWidth = sel ? 2 : 1;
        g.stroke();
      }
      g.beginPath();
      g.arc(ax, ay, sel ? 4 : 3, 0, Math.PI * 2);
      g.fillStyle = color;
      g.fill();
      if (sel || i === hoverBone) {
        g.fillStyle = color;
        g.font = "11px sans-serif";
        g.fillText(b.name, ax + 6, ay - 6);
      }
    });
    if (skel.length === 0) {
      g.fillStyle = $themeColors.faint;
      g.font = "12px sans-serif";
      g.fillText(
        isRig ? "No bones yet - add one." : "No skeleton to preview on: open a scene with one, or add a .rig.",
        12,
        20,
      );
    }
  });

  let drag: { bone: number; move: boolean } | null = null;
  let pan: { x: number; y: number; cx: number; cy: number } | null = null;

  function pointer(e: PointerEvent): [number, number] {
    const r = canvas!.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }

  function onPreviewDown(e: PointerEvent) {
    const [x, y] = pointer(e);
    if (e.button === 1 || e.button === 2) {
      pan = { x, y, cx: view?.cx ?? 0, cy: view?.cy ?? 0 };
      canvas!.setPointerCapture(e.pointerId);
      return;
    }
    const i = boneAt(world, toWorld(x, y), 8 / (view?.scale ?? 1));
    if (i < 0) {
      selBone = null;
      return;
    }
    selBone = posed[i].name;
    playing = false;
    drag = { bone: i, move: e.shiftKey };
    canvas!.setPointerCapture(e.pointerId);
  }

  function onPreviewMove(e: PointerEvent) {
    const [x, y] = pointer(e);
    if (pan && view) {
      view = { ...view, cx: pan.cx - (x - pan.x) / view.scale, cy: pan.cy + (y - pan.y) / view.scale };
      return;
    }
    if (!drag) {
      hoverBone = boneAt(world, toWorld(x, y), 8 / (view?.scale ?? 1));
      return;
    }
    const p = toWorld(x, y);
    const i = drag.bone;
    const name = posed[i].name;
    if (doc.clip) {
      const at = snap(t, snapStep);
      t = at;
      const value = drag.move ? positionAt(posed, world, i, p) : rotationToward(posed, world, i, p);
      edit({ clip: setKey(doc.clip, name, drag.move ? "position" : "rotation", at, value).clip });
    } else if (doc.rig) {
      const patch = drag.move ? { position: positionAt(posed, world, i, p) } : { rotation: rotationToward(posed, world, i, p) };
      edit({ rig: setBone(doc.rig, name, patch) });
    }
  }

  function onPreviewUp() {
    drag = null;
    pan = null;
  }

  function onWheel(e: WheelEvent) {
    if (!view) return;
    e.preventDefault();
    const [x, y] = [e.offsetX, e.offsetY];
    const before = toWorld(x, y);
    const scale = view.scale * Math.exp(-e.deltaY * 0.0015);
    view = { ...view, scale };
    const after = toWorld(x, y);
    view = { ...view, cx: view.cx + before[0] - after[0], cy: view.cy + before[1] - after[1] };
  }

  // ── The dope sheet ──

  let sheet = $state<HTMLDivElement | null>(null);
  let sheetW = $state(600);
  $effect(() => {
    const el = sheet;
    if (!el) return;
    const ro = new ResizeObserver(() => (sheetW = el.clientWidth));
    ro.observe(el);
    return () => ro.disconnect();
  });
  const LABEL_W = 130;
  const PAD = 8;
  const trackW = $derived(Math.max(sheetW - LABEL_W - PAD * 2, 50));
  const xOf = (time: number) => PAD + (time / duration) * trackW;
  const timeOf = (x: number) => ((x - PAD) / trackW) * duration;

  let ticks = $derived.by(() => {
    const step = [0.01, 0.02, 0.05, 0.1, 0.25, 0.5, 1, 2, 5, 10].find((s) => (s / duration) * trackW >= 40) ?? 10;
    const out: number[] = [];
    for (let v = 0; v <= duration + 1e-6; v += step) out.push(Math.round(v * 1000) / 1000);
    return out;
  });

  let scrubbing = false;
  function trackX(e: PointerEvent): number {
    return e.clientX - (e.currentTarget as HTMLElement).getBoundingClientRect().left;
  }
  function onRulerDown(e: PointerEvent) {
    scrubbing = true;
    playing = false;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    t = Math.min(snap(timeOf(trackX(e)), snapStep), duration);
  }
  function onRulerMove(e: PointerEvent) {
    if (scrubbing) t = Math.min(snap(timeOf(trackX(e)), snapStep), duration);
  }

  let keyDrag: { bone: string; channel: Channel | null; k: number; from: number; startX: number; moved: boolean } | null =
    null;

  function onKeyDown(e: PointerEvent, bone: string, channel: Channel | null, k: number, time: number) {
    e.stopPropagation();
    selBone = bone;
    selKey = { bone, channel, k, time };
    t = time;
    playing = false;
    keyDrag = { bone, channel, k, from: time, startX: e.clientX, moved: false };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }
  function onKeyMove(e: PointerEvent) {
    if (!keyDrag || !doc.clip) return;
    if (!keyDrag.moved && Math.abs(e.clientX - keyDrag.startX) < 3) return;
    keyDrag.moved = true;
    const to = Math.min(snap(keyDrag.from + ((e.clientX - keyDrag.startX) / trackW) * duration, snapStep), duration);
    const cur = selKey?.time ?? keyDrag.from;
    if (Math.abs(to - cur) < 1e-6) return;
    if (keyDrag.channel) {
      const r = moveKey(doc.clip, keyDrag.bone, keyDrag.channel, selKey!.k, to);
      edit({ clip: r.clip });
      selKey = { bone: keyDrag.bone, channel: keyDrag.channel, k: r.index, time: to };
    } else {
      edit({ clip: moveBoneKeys(doc.clip, keyDrag.bone, cur, to) });
      selKey = { bone: keyDrag.bone, channel: null, k: 0, time: to };
    }
    t = to;
  }
  function onKeyUp() {
    keyDrag = null;
  }

  function addKeyAt(e: MouseEvent, bone: string, channel: Channel) {
    if (!doc.clip) return;
    const x = e.clientX - (e.currentTarget as HTMLElement).getBoundingClientRect().left;
    const at = Math.min(snap(timeOf(x), snapStep), duration);
    const r = setKey(doc.clip, bone, channel, at, valueAt(skel, doc.clip, bone, channel, at));
    edit({ clip: r.clip });
    selBone = bone;
    selKey = { bone, channel, k: r.index, time: at };
    t = at;
  }

  function deleteSelectedKey() {
    if (!doc.clip || !selKey) return;
    edit({
      clip: selKey.channel
        ? removeKey(doc.clip, selKey.bone, selKey.channel, selKey.k)
        : removeBoneKeys(doc.clip, selKey.bone, selKey.time),
    });
    selKey = null;
  }

  function keySelected() {
    if (!doc.clip || !selBone) return;
    const tr = trackFor(doc.clip, selBone);
    const channels = CHANNELS.filter((c) => (tr?.[c]?.length ?? 0) > 0);
    edit({ clip: keyPose(doc.clip, skel, selBone, snap(t, snapStep), channels.length > 0 ? channels : ["rotation"]) });
  }

  function keyAll() {
    if (!doc.clip) return;
    let c = doc.clip;
    for (const b of skel) c = keyPose(c, skel, b.name, snap(t, snapStep), ["rotation"]);
    edit({ clip: c });
  }

  // The selected channel key, for the fields under the sheet.
  let selValue = $derived.by(() => {
    if (!doc.clip || !selKey?.channel) return null;
    const keys = (trackFor(doc.clip, selKey.bone)?.[selKey.channel] ?? []) as { time: number; value: unknown }[];
    return keys[selKey.k] ?? null;
  });

  function setSelValue(value: number | [number, number]) {
    if (!doc.clip || !selKey?.channel || !selValue) return;
    edit({ clip: setKey(doc.clip, selKey.bone, selKey.channel, selValue.time, value).clip });
  }
  function setSelTime(time: number) {
    if (!doc.clip || !selKey?.channel) return;
    const r = moveKey(doc.clip, selKey.bone, selKey.channel, selKey.k, Math.min(Math.max(time, 0), duration));
    edit({ clip: r.clip });
    selKey = { ...selKey, k: r.index, time };
  }

  const deg = (r: number) => Math.round(((r * 180) / Math.PI) * 100) / 100;
  const rad = (d: number) => (d * Math.PI) / 180;
  const num = (e: Event) => Number((e.target as HTMLInputElement).value);

  // ── Rig fields ──

  let rigTree = $derived.by(() => {
    const rig = doc.rig;
    if (!rig) return [];
    const out: { name: string; depth: number }[] = [];
    const names = new Set(rig.bones.map((b) => b.name));
    const visit = (parent: string | undefined, depth: number) => {
      for (const b of rig.bones) {
        const p = b.parent && names.has(b.parent) ? b.parent : undefined;
        if (p === parent && !out.some((o) => o.name === b.name)) {
          out.push({ name: b.name, depth });
          visit(b.name, depth + 1);
        }
      }
    };
    visit(undefined, 0);
    for (const b of rig.bones) if (!out.some((o) => o.name === b.name)) out.push({ name: b.name, depth: 0 });
    return out;
  });
  let selRigBone = $derived(doc.rig?.bones.find((b) => b.name === selBone) ?? null);

  function patchBone(patch: Record<string, unknown>) {
    if (!doc.rig || !selBone) return;
    edit({ rig: setBone(doc.rig, selBone, patch) });
  }

  // ── File ──

  async function save() {
    history.flush();
    const text = doc.rig ? formatRig($state.snapshot(doc.rig) as Rig) : formatClip($state.snapshot(doc.clip) as SkelClip);
    try {
      await writeTextFile(`${projectPath}/${relativePath}`, text);
      dirty = false;
      logInfo(`Saved ${relativePath}`);
      requestAssetRefresh();
      // The viewport loads clips and rigs once a session; hand it this one,
      // so the next Play uses what was saved.
      try {
        const wasm: any = await import("./viewport-wasm/jengine_editor_viewport.js");
        if (doc.rig) wasm.viewport_load_skeleton_rig?.(clipId(relativePath), text);
        else wasm.viewport_load_skeletal_clip?.(clipId(relativePath), text);
      } catch {
        // No viewport yet: it loads every file when it starts.
      }
    } catch (e) {
      logError(`Could not save ${relativePath}: ${e}`);
    }
  }

  async function openAsText() {
    try {
      await openInCodeEditor(projectPath, relativePath, $project?.settings.code_editor ?? "");
    } catch (e) {
      logError(`${e}`);
    }
  }

  function close() {
    if (dirty && !confirm(`Discard the changes to ${relativePath}?`)) return;
    playing = false;
    onclose();
  }

  // Keys reach the window from the start.
  let overlay = $state<HTMLDivElement | null>(null);
  $effect(() => overlay?.focus());

  function onKeydown(e: KeyboardEvent) {
    const target = e.target as HTMLElement;
    if (target.tagName === "INPUT" || target.tagName === "SELECT" || target.tagName === "TEXTAREA") return;
    // A focused button takes its own Space and Enter.
    if (target.tagName === "BUTTON" && (e.key === " " || e.key === "Enter")) return;
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
      e.preventDefault();
      void save();
    } else if (e.key === "Delete" || e.key === "Backspace") {
      if (doc.clip && selKey) deleteSelectedKey();
      else if (doc.rig && selBone) {
        edit({ rig: removeBone(doc.rig, selBone) });
        selBone = null;
      } else return;
      e.preventDefault();
    } else if (e.key === "k" || e.key === "K") {
      keySelected();
    } else if (e.key === " " && doc.clip) {
      e.preventDefault();
      playing = !playing;
    } else if (e.key === "Escape") {
      close();
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="overlay" role="dialog" aria-label="Skeletal editor" tabindex="-1" bind:this={overlay} onkeydown={onKeydown}>
  <div class="window">
    <div class="head">
      <span class="title">{relativePath.split("/").pop()}{dirty ? " *" : ""}</span>
      {#if doc.clip}
        <label class="inline">Name <input value={doc.clip.name} onchange={(e) => edit({ clip: { ...doc.clip!, name: (e.target as HTMLInputElement).value } })} /></label>
        <label class="inline"
          >Length <input
            type="number"
            min="0.01"
            step="0.05"
            value={doc.clip.duration}
            onchange={(e) => edit({ clip: { ...doc.clip!, duration: Math.max(num(e), 0.01) } })}
          />s</label
        >
        <label class="inline">
          Preview on
          <select value={source?.label ?? ""} onchange={(e) => (chosenLabel = (e.target as HTMLSelectElement).value)}>
            {#each sources as s}
              <option value={s.label}>{s.label} ({coverage(s.skel, trackBones)}/{trackBones.length})</option>
            {/each}
          </select>
        </label>
      {:else if doc.rig}
        <label class="inline">Name <input value={doc.rig.name} onchange={(e) => edit({ rig: { ...doc.rig!, name: (e.target as HTMLInputElement).value } })} /></label>
      {/if}
      <span class="spacer"></span>
      <button onclick={frame} title="Frame the skeleton">Frame</button>
      <button onclick={openAsText} title="Open the file in the code editor">Open as text</button>
      <button class="save" disabled={!dirty} onclick={save}>Save</button>
      <button onclick={close} title="Close">×</button>
    </div>
    {#if error}<div class="error">{error}</div>{/if}
    <div class="body" class:rig={isRig}>
      <canvas
        bind:this={canvas}
        class="preview"
        onpointerdown={onPreviewDown}
        onpointermove={onPreviewMove}
        onpointerup={onPreviewUp}
        onpointerleave={() => (hoverBone = -1)}
        onwheel={onWheel}
        oncontextmenu={(e) => e.preventDefault()}
      ></canvas>
      {#if doc.rig}
        <div class="bones">
          <div class="bone-actions">
            <button onclick={() => { const r = addBone(doc.rig!, selBone); edit({ rig: r.rig }); selBone = r.name; }}>
              + {selBone ? "Child" : "Root"} bone
            </button>
            <button disabled={!selBone} onclick={() => { edit({ rig: removeBone(doc.rig!, selBone!) }); selBone = null; }}>Remove</button>
          </div>
          <div class="tree">
            {#each rigTree as b}
              <button class="tree-row" class:sel={b.name === selBone} style="padding-left:{6 + b.depth * 14}px" onclick={() => (selBone = b.name)}>
                {b.name}
              </button>
            {/each}
          </div>
          {#if selRigBone}
            {@const b = selRigBone}
            <div class="fields">
              <label>Name <input value={b.name} onchange={(e) => { const to = (e.target as HTMLInputElement).value.trim(); const r = renameBone(doc.rig!, b.name, to); if (r !== doc.rig) { edit({ rig: r }); selBone = to; } }} /></label>
              <label>
                Parent
                <select value={b.parent ?? ""} onchange={(e) => patchBone({ parent: (e.target as HTMLSelectElement).value || undefined })}>
                  <option value="">(none - a root)</option>
                  {#each doc.rig.bones.filter((o) => o.name !== b.name) as o}<option value={o.name}>{o.name}</option>{/each}
                </select>
              </label>
              <label>Position
                <input type="number" value={b.position?.[0] ?? 0} onchange={(e) => patchBone({ position: [num(e), b.position?.[1] ?? 0] })} />
                <input type="number" value={b.position?.[1] ?? 0} onchange={(e) => patchBone({ position: [b.position?.[0] ?? 0, num(e)] })} />
              </label>
              <label>Rotation° <input type="number" step="1" value={deg(b.rotation ?? 0)} onchange={(e) => patchBone({ rotation: rad(num(e)) })} /></label>
              <label>Scale
                <input type="number" step="0.1" value={b.scale?.[0] ?? 1} onchange={(e) => patchBone({ scale: [num(e), b.scale?.[1] ?? 1] })} />
                <input type="number" step="0.1" value={b.scale?.[1] ?? 1} onchange={(e) => patchBone({ scale: [b.scale?.[0] ?? 1, num(e)] })} />
              </label>
              <label>Length <input type="number" min="0" value={b.length ?? 1} onchange={(e) => patchBone({ length: Math.max(num(e), 0) })} /></label>
            </div>
          {/if}
          <div class="hint">Drag a bone to turn it, Shift-drag to move its joint; right-drag pans, the wheel zooms.</div>
        </div>
      {/if}
    </div>

    {#if doc.clip}
      <div class="transport">
        <button onclick={() => (playing = !playing)} title="Play / pause (Space)">{playing ? "⏸" : "▶"}</button>
        <span class="time">{t.toFixed(3)}s</span>
        <label class="inline">
          Snap
          <select value={String(snapStep)} onchange={(e) => (snapStep = Number((e.target as HTMLSelectElement).value))}>
            <option value="0">off</option>
            <option value={String(1 / 60)}>1/60 s</option>
            <option value={String(1 / 30)}>1/30 s</option>
            <option value="0.05">0.05 s</option>
            <option value="0.1">0.1 s</option>
          </select>
        </label>
        <button disabled={!selBone} onclick={keySelected} title="Key the selected bone's pose at the playhead (K)">Key bone</button>
        <button disabled={skel.length === 0} onclick={keyAll} title="Key every bone's rotation at the playhead">Key all</button>
        <button
          disabled={!selBone || !trackFor(doc.clip, selBone)}
          onclick={() => { edit({ clip: removeTrack(doc.clip!, selBone!) }); selKey = null; }}
          title="Remove every key of the selected bone">Clear bone</button
        >
        <span class="spacer"></span>
        {#if selKey?.channel && selValue}
          {@const v = selValue.value}
          <span class="dim">{selKey.bone}.{selKey.channel}</span>
          <label class="inline">at <input type="number" step="0.05" value={selValue.time} onchange={(e) => setSelTime(num(e))} /></label>
          {#if typeof v === "number"}
            <label class="inline">° <input type="number" value={deg(v)} onchange={(e) => setSelValue(rad(num(e)))} /></label>
          {:else if Array.isArray(v)}
            <input type="number" value={v[0]} onchange={(e) => setSelValue([num(e), v[1] as number])} />
            <input type="number" value={v[1]} onchange={(e) => setSelValue([v[0] as number, num(e)])} />
          {/if}
        {:else}
          <span class="dim">Drag a bone to key it; double-click a channel row to add a key.</span>
        {/if}
      </div>
      <div class="sheet" bind:this={sheet}>
        <div class="srow ruler">
          <div class="label"></div>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div class="lane" onpointerdown={onRulerDown} onpointermove={onRulerMove} onpointerup={() => (scrubbing = false)}>
            {#each ticks as tk}
              <span class="tick" style="left:{xOf(tk)}px">{tk}</span>
            {/each}
          </div>
        </div>
        <div class="rows">
          {#each rows as bone}
            {@const tr = trackFor(doc.clip, bone)}
            <div class="srow" class:sel={bone === selBone}>
              <button class="label" onclick={() => (selBone = selBone === bone ? null : bone)}>
                {bone === selBone ? "▾" : "▸"} {bone}{skel.some((b) => b.name === bone) ? "" : " (not in skeleton)"}
              </button>
              <div class="lane">
                {#each keyTimes(tr) as time}
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <span
                    class="key summary"
                    class:on={selKey?.bone === bone && selKey.channel === null && Math.abs(selKey.time - time) < 1e-4}
                    style="left:{xOf(time)}px"
                    onpointerdown={(e) => onKeyDown(e, bone, null, 0, time)}
                    onpointermove={onKeyMove}
                    onpointerup={onKeyUp}
                  ></span>
                {/each}
              </div>
            </div>
            {#if bone === selBone}
              {#each CHANNELS as channel}
                {@const keys = (tr?.[channel] ?? []) as { time: number }[]}
                <div class="srow sub">
                  <div class="label">{channel}</div>
                  <!-- svelte-ignore a11y_no_static_element_interactions -->
                  <div class="lane" ondblclick={(e) => addKeyAt(e, bone, channel)}>
                    {#each keys as key, k}
                      <!-- svelte-ignore a11y_no_static_element_interactions -->
                      <span
                        class="key {channel}"
                        class:on={selKey?.bone === bone && selKey.channel === channel && selKey.k === k}
                        style="left:{xOf(key.time)}px"
                        onpointerdown={(e) => onKeyDown(e, bone, channel, k, key.time)}
                        onpointermove={onKeyMove}
                        onpointerup={onKeyUp}
                      ></span>
                    {/each}
                  </div>
                </div>
              {/each}
            {/if}
          {/each}
          <div class="playhead" style="left:{LABEL_W + xOf(t)}px"></div>
        </div>
      </div>
    {/if}
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.55);
    z-index: 1000;
    display: flex;
    align-items: center;
    justify-content: center;
    outline: none;
  }
  .window {
    width: min(1100px, 96vw);
    height: min(820px, 92vh);
    display: flex;
    flex-direction: column;
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    border-radius: 6px;
    color: var(--ed-text);
    font-size: 12px;
  }
  .head,
  .transport {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--ed-line);
    flex-wrap: wrap;
  }
  .title {
    font-weight: 600;
  }
  .spacer {
    flex: 1;
  }
  .inline {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--ed-muted);
  }
  .inline input {
    width: 70px;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .preview {
    flex: 1;
    min-width: 0;
    width: 100%;
    height: 100%;
    cursor: crosshair;
    touch-action: none;
  }
  .bones {
    width: 280px;
    border-left: 1px solid var(--ed-line);
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 6px;
    overflow-y: auto;
  }
  .bone-actions {
    display: flex;
    gap: 6px;
  }
  .tree {
    display: flex;
    flex-direction: column;
    max-height: 40%;
    overflow-y: auto;
    border: 1px solid var(--ed-line);
  }
  .tree-row {
    text-align: left;
    border: none;
    border-radius: 0;
  }
  .tree-row.sel,
  .srow.sel > .label {
    background: var(--ed-line);
  }
  .fields {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .fields label {
    display: flex;
    align-items: center;
    gap: 4px;
    color: var(--ed-muted);
  }
  .fields input,
  .fields select {
    flex: 1;
    min-width: 0;
  }
  .hint,
  .dim {
    color: var(--ed-faint);
  }
  .transport input {
    width: 64px;
  }
  .time {
    font-family: var(--ed-mono);
    min-width: 60px;
  }
  .sheet {
    height: 38%;
    min-height: 140px;
    display: flex;
    flex-direction: column;
    overflow: hidden;
  }
  .rows {
    position: relative;
    flex: 1;
    overflow-y: auto;
  }
  .srow {
    display: flex;
    height: 20px;
    border-bottom: 1px solid var(--ed-hover);
  }
  .srow.sub {
    background: var(--ed-mantle);
  }
  .label {
    width: 130px;
    box-sizing: border-box;
    flex-shrink: 0;
    text-align: left;
    padding: 0 6px;
    border: none;
    border-radius: 0;
    background: none;
    color: var(--ed-text);
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    font-size: 11px;
    line-height: 20px;
  }
  .srow.sub .label {
    padding-left: 22px;
    color: var(--ed-muted);
  }
  .lane {
    position: relative;
    flex: 1;
  }
  .ruler {
    height: 22px;
    border-bottom: 1px solid var(--ed-line);
  }
  .ruler .lane {
    cursor: ew-resize;
  }
  .tick {
    position: absolute;
    top: 3px;
    font-size: 10px;
    color: var(--ed-faint);
    border-left: 1px solid var(--ed-line-strong);
    padding-left: 2px;
  }
  .key {
    position: absolute;
    top: 5px;
    width: 9px;
    height: 9px;
    margin-left: -5px;
    transform: rotate(45deg);
    background: var(--ed-line-strong);
    cursor: pointer;
  }
  .key.summary {
    background: var(--ed-muted);
  }
  .key.rotation {
    background: var(--ed-accent);
  }
  .key.position {
    background: var(--ed-ok);
  }
  .key.scale {
    background: var(--ed-a3);
  }
  .key.on {
    outline: 2px solid var(--ed-warn);
  }
  .playhead {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 1px;
    background: var(--ed-err);
    pointer-events: none;
  }
  .error {
    color: var(--ed-err);
    padding: 6px 10px;
  }
  input,
  select,
  button {
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    border-radius: 4px;
    font-size: 11.5px;
    padding: 3px 5px;
  }
  button {
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .save:not(:disabled) {
    border-color: var(--ed-ok);
  }
</style>
