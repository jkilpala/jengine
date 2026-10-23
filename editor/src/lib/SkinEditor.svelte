<script lang="ts">
  import { themeColors, withAlpha } from "./stores/themeColors";
  // The skin editor: a character rigged from one picture. Opened on a scene
  // skeleton (its root's inspector, *Skin & rig*) or on a picture (the asset
  // browser's *Rig as a character*, which makes the root on Apply).
  //
  // Four steps, one tab each:
  //   Bones   - drag on empty space to draw a bone (a child of the selected
  //             one), drag a bone to turn it, its tip to turn and stretch
  //             it, Shift-drag to move its joint, Delete to remove it.
  //   Mesh    - cover the picture's opaque pixels with a grid of triangles.
  //   Weights - each vertex weighted to the bones near it, then a brush to
  //             paint the selected bone's weight up, down or smooth.
  //   Pose    - turn bones to see the skin bend; nothing here is saved.
  // Apply writes the bones as entities under the root and the root's
  // `skinned_mesh`, as one scene undo step. Inside the editor, Ctrl+Z
  // undoes its own steps.
  //
  // The geometry is `skinEdit.ts`; its skinning is checked against the
  // engine's (`skin_sample_matches_the_editor`).
  import { get } from "svelte/store";
  import { project } from "./stores/projectStore";
  import { projectAssets } from "./stores/projectAssetsStore";
  import { scene } from "./stores/sceneStore";
  import { execute } from "./stores/commandStore";
  import { selectEntity } from "./stores/selectionStore";
  import { readAssetBinary } from "./api/project";
  import { assetsWithExtensions } from "./fieldValues";
  import { logError, logInfo } from "./stores/consoleStore";
  import { SettledHistory } from "./settledHistory";
  import { claimUndo } from "./stores/undoTargetStore";
  import {
    apply as applyAffine,
    boneAt,
    removeBone,
    renameBone,
    rotationToward,
    positionAt,
    sceneRigBones,
    setBone,
    IDENTITY,
    type Affine,
    type Rig,
    skeletonFromRig,
    worldPose,
    freshBoneName,
    type RigBone,
    type SkelBone,
  } from "./skelEdit";
  import {
    autoWeights,
    boneFrames,
    brushHits,
    centredPlacement,
    fromSkinnedMesh,
    gridMesh,
    handOver,
    invert,
    paintWeights,
    placementOf,
    renameWeights,
    segmentDistance,
    segmentsOf,
    skin,
    smoothWeights,
    toSkinnedMesh,
    type Placement,
    type SkinMesh,
    type SkinnedMeshData,
  } from "./skinEdit";

  let {
    rootIndex,
    texture: initialTexture = "",
    onclose,
  }: { rootIndex: number; texture?: string; onclose: () => void } = $props();

  type Bone = RigBone & { entity: string };
  interface Doc {
    bones: Bone[];
    texture: string;
    place: Placement | null;
    mesh: SkinMesh | null;
    /** Scene entities of bones removed here, to remove on Apply. */
    removed: string[];
  }

  const start = get(scene);
  const startRoot = start.entities[rootIndex];
  let rootName = $state(startRoot?.name ?? "");
  const existingSkin = (startRoot?.components as any)?.skinned_mesh as SkinnedMeshData | undefined;

  function initialDoc(): Doc {
    const bones: Bone[] = rootIndex >= 0 ? sceneRigBones(start.entities as any, rootIndex) : [];
    const skel = skeletonFromRig({ name: "", bones });
    return {
      bones,
      texture: existingSkin?.texture_id ?? initialTexture,
      place: null,
      mesh: existingSkin && (existingSkin.vertices?.length ?? 0) > 0 ? fromSkinnedMesh(existingSkin, skel) : null,
      removed: [],
    };
  }
  let doc = $state<Doc>(initialDoc());
  let dirty = $state(false);

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

  function edit(next: Partial<Doc>) {
    doc = { ...doc, ...next };
    dirty = true;
  }

  // The rig edits (`skelEdit`) work on a rig - a plain copy, since they
  // clone it and a state proxy cannot be cloned; the bones keep their
  // entity.
  const asRig = (bones: Bone[]): Rig => ({ name: "", bones: $state.snapshot(bones) as Bone[] });
  const patchBone = (name: string, patch: Partial<RigBone>) =>
    edit({ bones: setBone(asRig(doc.bones), name, patch).bones as Bone[] });

  type Mode = "bones" | "mesh" | "weights" | "pose";
  let mode = $state<Mode>(doc.mesh ? "weights" : "bones");

  // ── The picture ──

  let img = $state<HTMLImageElement | null>(null);
  let pixels: Uint8ClampedArray | null = null;
  let imgW = $state(0);
  let imgH = $state(0);
  let pictures = $derived(assetsWithExtensions($projectAssets, ["png", "jpg", "jpeg", "webp"]));

  $effect(() => {
    void loadPicture(doc.texture);
  });

  async function loadPicture(tex: string) {
    img = null;
    pixels = null;
    const proj = get(project);
    if (!tex || !proj) return;
    try {
      const bytes = new Uint8Array(await readAssetBinary(proj.path, tex.replace(/^assets\//, "")));
      const url = URL.createObjectURL(new Blob([bytes]));
      const el = new Image();
      el.src = url;
      await el.decode();
      const c = document.createElement("canvas");
      c.width = el.naturalWidth;
      c.height = el.naturalHeight;
      const g = c.getContext("2d")!;
      g.drawImage(el, 0, 0);
      pixels = g.getImageData(0, 0, c.width, c.height).data;
      imgW = el.naturalWidth;
      imgH = el.naturalHeight;
      img = el;
      // Where the picture sits: the mesh's own placement, else centred.
      if (!doc.place) {
        const place = (doc.mesh && placementOf(doc.mesh, imgW, imgH)) || centredPlacement(imgW, imgH);
        doc = { ...doc, place };
        history.reset();
      }
      if (cellSize === 0) cellSize = Math.max(6, Math.round(Math.max(imgW, imgH) / 24));
      frame();
    } catch (e) {
      logError(`Could not read ${tex}: ${e}`);
    }
  }

  // ── The skeleton ──

  let skel = $derived<SkelBone[]>(skeletonFromRig({ name: "", bones: doc.bones }));
  let pose = $state<Record<string, number>>({});
  let posed = $derived<SkelBone[]>(
    skel.map((b) => (b.name in pose ? { ...b, local: { ...b.local, rotation: pose[b.name] } } : b)),
  );
  let shown = $derived(mode === "pose" ? posed : skel);
  let world = $derived(worldPose(shown));
  let segments = $derived(segmentsOf(skel));

  let drawnVertices = $derived.by(() => {
    const m = doc.mesh;
    if (!m) return [];
    return mode === "pose" ? skin(m, boneFrames(skel), boneFrames(posed)) : m.vertices;
  });

  let selBone = $state<string | null>(null);
  /** Bones mode's tool: draw new bones, or edit the ones there are. */
  let boneTool = $state<"draw" | "edit">(doc.bones.length === 0 ? "draw" : "edit");

  // ── Tools ──

  let cellSize = $state(0);
  let alphaCut = $state(10);
  let influences = $state(2);
  let brushRadius = $state(40);
  let brushStrength = $state(0.4);
  let brushMode = $state<"add" | "remove" | "smooth">("add");

  function generateMesh() {
    if (!pixels || !doc.place) return;
    if (doc.mesh && !confirm("Replace the mesh? Its painted weights go with it.")) return;
    const mesh = gridMesh(pixels, imgW, imgH, cellSize, alphaCut, doc.place);
    mesh.weights = autoWeights(mesh.vertices, segments, influences);
    edit({ mesh });
    logInfo(`Skin mesh: ${mesh.vertices.length} vertices, ${mesh.indices.length / 3} triangles`);
  }

  function weighAll() {
    if (!doc.mesh) return;
    if (!confirm("Weigh every vertex again from the bones? Painted weights are replaced.")) return;
    edit({ mesh: { ...doc.mesh, weights: autoWeights(doc.mesh.vertices, segments, influences) } });
  }

  /** The nearest bone to vertex `i` other than `bone` - who takes weight a
   *  brush removes from a vertex's only bone. */
  function heirOf(i: number, bone: string): string | null {
    const v = doc.mesh?.vertices[i];
    if (!v) return null;
    let best: string | null = null;
    let bestD = Infinity;
    for (const s of segments) {
      if (s.name === bone) continue;
      const d = segmentDistance(v, s.origin, s.tip);
      if (d < bestD) {
        bestD = d;
        best = s.name;
      }
    }
    return best;
  }

  // ── The view ──

  let canvas = $state<HTMLCanvasElement | null>(null);
  let cw = $state(600);
  let ch = $state(400);
  let view = $state({ cx: 0, cy: 0, scale: 1 });
  let viewMoved = false;

  function frame() {
    const xs: number[] = [];
    const ys: number[] = [];
    if (doc.place && imgW > 0) {
      xs.push(doc.place.x, doc.place.x + imgW * doc.place.scale);
      ys.push(doc.place.y, doc.place.y - imgH * doc.place.scale);
    }
    for (const s of segments) {
      xs.push(s.origin[0], s.tip[0]);
      ys.push(s.origin[1], s.tip[1]);
    }
    if (xs.length === 0) {
      view = { cx: 0, cy: 0, scale: 1 };
      return;
    }
    const x0 = Math.min(...xs);
    const x1 = Math.max(...xs);
    const y0 = Math.min(...ys);
    const y1 = Math.max(...ys);
    view = {
      cx: (x0 + x1) / 2,
      cy: (y0 + y1) / 2,
      scale: Math.min(cw / (Math.max(x1 - x0, 20) * 1.2), ch / (Math.max(y1 - y0, 20) * 1.2)),
    };
  }

  const toScreen = (p: [number, number]): [number, number] => [
    cw / 2 + (p[0] - view.cx) * view.scale,
    ch / 2 - (p[1] - view.cy) * view.scale,
  ];
  const toWorld = (x: number, y: number): [number, number] => [
    (x - cw / 2) / view.scale + view.cx,
    -(y - ch / 2) / view.scale + view.cy,
  ];

  $effect(() => {
    const el = canvas;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      cw = el.clientWidth;
      ch = el.clientHeight;
      // Framed at the real size until the view is moved by hand.
      if (!viewMoved) frame();
    });
    ro.observe(el);
    return () => ro.disconnect();
  });

  let hover = $state<[number, number] | null>(null);
  let ghost = $state<{ from: [number, number]; to: [number, number] } | null>(null);

  /** Draw one triangle of the picture, mapped from its pixels to the
   *  screen (grown half a pixel, or neighbours show a hairline seam). */
  function texturedTriangle(
    g: CanvasRenderingContext2D,
    image: HTMLImageElement,
    s: [number, number][],
    t: [number, number][],
  ) {
    const cx = (s[0][0] + s[1][0] + s[2][0]) / 3;
    const cy = (s[0][1] + s[1][1] + s[2][1]) / 3;
    g.save();
    g.beginPath();
    s.forEach(([x, y], k) => {
      const d = Math.hypot(x - cx, y - cy) || 1;
      const gx = x + ((x - cx) / d) * 0.9;
      const gy = y + ((y - cy) / d) * 0.9;
      if (k === 0) g.moveTo(gx, gy);
      else g.lineTo(gx, gy);
    });
    g.closePath();
    g.clip();
    const [[x0, y0], [x1, y1], [x2, y2]] = s;
    const [[u0, v0], [u1, v1], [u2, v2]] = t;
    const du1 = u1 - u0;
    const dv1 = v1 - v0;
    const du2 = u2 - u0;
    const dv2 = v2 - v0;
    const det = du1 * dv2 - du2 * dv1;
    if (Math.abs(det) > 1e-9) {
      const a = ((x1 - x0) * dv2 - (x2 - x0) * dv1) / det;
      const b = ((y1 - y0) * dv2 - (y2 - y0) * dv1) / det;
      const c = ((x2 - x0) * du1 - (x1 - x0) * du2) / det;
      const d = ((y2 - y0) * du1 - (y1 - y0) * du2) / det;
      g.transform(a, b, c, d, x0 - a * u0 - c * v0, y0 - b * u0 - d * v0);
      g.drawImage(image, 0, 0);
    }
    g.restore();
  }

  const heat = (w: number) => {
    // Blue (none) through green to red (all).
    const h = (1 - Math.min(1, Math.max(0, w))) * 240;
    return `hsla(${h}, 90%, 55%, 0.55)`;
  };

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
    const [ox, oy] = toScreen([0, 0]);
    g.strokeStyle = $themeColors.line;
    g.beginPath();
    g.moveTo(0, oy);
    g.lineTo(cw, oy);
    g.moveTo(ox, 0);
    g.lineTo(ox, ch);
    g.stroke();

    const mesh = doc.mesh;
    const place = doc.place;
    const verts = drawnVertices.map(toScreen);
    // The picture: bent by the bones in Pose, flat otherwise.
    if (img && place) {
      if (mode === "pose" && mesh) {
        const tex = mesh.uvs.map(([u, v]) => [u * imgW, v * imgH] as [number, number]);
        for (let t = 0; t + 2 < mesh.indices.length; t += 3) {
          const [a, b, c] = [mesh.indices[t], mesh.indices[t + 1], mesh.indices[t + 2]];
          texturedTriangle(g, img, [verts[a], verts[b], verts[c]], [tex[a], tex[b], tex[c]]);
        }
      } else {
        const [sx, sy] = toScreen([place.x, place.y]);
        g.globalAlpha = mode === "weights" ? 0.35 : 1;
        g.imageSmoothingEnabled = view.scale * place.scale < 2;
        g.drawImage(img, sx, sy, imgW * place.scale * view.scale, imgH * place.scale * view.scale);
        g.globalAlpha = 1;
      }
    }
    if (mesh) {
      if (mode === "weights" && selBone) {
        for (let t = 0; t + 2 < mesh.indices.length; t += 3) {
          const ids = [mesh.indices[t], mesh.indices[t + 1], mesh.indices[t + 2]];
          const w = ids.reduce((s, i) => s + (mesh.weights[i]?.[selBone!] ?? 0), 0) / 3;
          g.beginPath();
          g.moveTo(...verts[ids[0]]);
          g.lineTo(...verts[ids[1]]);
          g.lineTo(...verts[ids[2]]);
          g.closePath();
          g.fillStyle = heat(w);
          g.fill();
        }
      }
      if (mode === "mesh" || mode === "weights" || mode === "bones") {
        g.strokeStyle = mode === "mesh" ? withAlpha($themeColors.ok, 0.7) : withAlpha($themeColors.text, 0.18);
        g.lineWidth = 1;
        g.beginPath();
        for (let t = 0; t + 2 < mesh.indices.length; t += 3) {
          const [a, b, c] = [verts[mesh.indices[t]], verts[mesh.indices[t + 1]], verts[mesh.indices[t + 2]]];
          g.moveTo(...a);
          g.lineTo(...b);
          g.lineTo(...c);
          g.closePath();
        }
        g.stroke();
      }
    }
    // The bones.
    world.forEach((w, i) => {
      const b = shown[i];
      const [ax, ay] = toScreen(w.origin);
      const [bx, by] = toScreen(w.tip);
      const len = Math.hypot(bx - ax, by - ay);
      const sel = b.name === selBone;
      const color = sel ? $themeColors.warn : $themeColors.accent;
      if (len > 1) {
        const ux = (bx - ax) / len;
        const uy = (by - ay) / len;
        const wdt = Math.min(Math.max(len * 0.1, 3), 9);
        const mx = ax + ux * Math.min(len * 0.2, 14);
        const my = ay + uy * Math.min(len * 0.2, 14);
        g.beginPath();
        g.moveTo(ax, ay);
        g.lineTo(mx - uy * wdt, my + ux * wdt);
        g.lineTo(bx, by);
        g.lineTo(mx + uy * wdt, my - ux * wdt);
        g.closePath();
        g.fillStyle = color + (mode === "weights" ? "33" : "55");
        g.fill();
        g.strokeStyle = color;
        g.lineWidth = sel ? 2 : 1;
        g.stroke();
      }
      g.beginPath();
      g.arc(ax, ay, 3, 0, Math.PI * 2);
      g.fillStyle = color;
      g.fill();
      if (sel) {
        g.font = "11px sans-serif";
        g.fillText(b.name, ax + 6, ay - 6);
      }
    });
    if (ghost) {
      const [ax, ay] = toScreen(ghost.from);
      const [bx, by] = toScreen(ghost.to);
      g.setLineDash([4, 3]);
      g.strokeStyle = $themeColors.warn;
      g.beginPath();
      g.moveTo(ax, ay);
      g.lineTo(bx, by);
      g.stroke();
      g.setLineDash([]);
    }
    if (mode === "weights" && hover) {
      g.strokeStyle = brushMode === "remove" ? $themeColors.err : brushMode === "smooth" ? $themeColors["cat-6"] : $themeColors.ok;
      g.beginPath();
      g.arc(hover[0], hover[1], brushRadius, 0, Math.PI * 2);
      g.stroke();
    }
    if (!img) {
      g.fillStyle = $themeColors.faint;
      g.font = "12px sans-serif";
      g.fillText("Pick the picture to rig (right).", 12, 20);
    }
  });

  // ── Pointer ──

  type Drag =
    | { kind: "turn" | "tip" | "move"; bone: number }
    | { kind: "draw"; from: [number, number] }
    | { kind: "paint" }
    | { kind: "pose"; bone: number }
    | { kind: "pan"; x: number; y: number; cx: number; cy: number };
  let drag: Drag | null = null;

  function local(e: PointerEvent): [number, number] {
    const r = canvas!.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  }

  function down(e: PointerEvent) {
    const [x, y] = local(e);
    const p = toWorld(x, y);
    try {
      canvas!.setPointerCapture(e.pointerId);
    } catch {
      // A pointer that cannot be captured (a pen leaving, a synthetic one).
    }
    if (e.button === 1 || e.button === 2) {
      viewMoved = true;
      drag = { kind: "pan", x, y, cx: view.cx, cy: view.cy };
      return;
    }
    const reach = 8 / view.scale;
    const hit = boneAt(world, p, reach);
    if (mode === "bones" && boneTool === "draw") {
      // Starting at a bone's tip carries on from it: a chain, joint to joint.
      const tip = world.findIndex((w) => Math.hypot(p[0] - w.tip[0], p[1] - w.tip[1]) < reach * 1.5);
      if (tip >= 0) selBone = shown[tip].name;
      const from: [number, number] = tip >= 0 ? [world[tip].tip[0], world[tip].tip[1]] : p;
      drag = { kind: "draw", from };
      ghost = { from, to: p };
    } else if (mode === "bones") {
      if (hit >= 0) {
        selBone = shown[hit].name;
        const tip = world[hit].tip;
        const nearTip = Math.hypot(p[0] - tip[0], p[1] - tip[1]) < reach * 1.5;
        drag = { kind: e.shiftKey ? "move" : nearTip ? "tip" : "turn", bone: hit };
      } else selBone = null;
    } else if (mode === "pose") {
      if (hit >= 0) {
        selBone = shown[hit].name;
        drag = { kind: "pose", bone: hit };
      }
    } else if (mode === "weights") {
      if (e.ctrlKey && hit >= 0) {
        selBone = shown[hit].name;
        return;
      }
      if (selBone && doc.mesh) {
        drag = { kind: "paint" };
        paintAt(p, e.shiftKey);
      }
    }
  }

  function move(e: PointerEvent) {
    const [x, y] = local(e);
    hover = [x, y];
    const p = toWorld(x, y);
    if (!drag) return;
    if (drag.kind === "pan") {
      view = { ...view, cx: drag.cx - (x - drag.x) / view.scale, cy: drag.cy + (y - drag.y) / view.scale };
    } else if (drag.kind === "draw") {
      ghost = { from: drag.from, to: p };
    } else if (drag.kind === "paint") {
      paintAt(p, e.shiftKey);
    } else if (drag.kind === "pose") {
      const name = posed[drag.bone].name;
      pose = { ...pose, [name]: rotationToward(posed, worldPose(posed), drag.bone, p) };
    } else {
      const i = drag.bone;
      const name = skel[i].name;
      const w = worldPose(skel);
      if (drag.kind === "move") {
        patchBone(name, { position: positionAt(skel, w, i, p) });
      } else {
        const patch: Partial<RigBone> = { rotation: rotationToward(skel, w, i, p) };
        if (drag.kind === "tip") {
          // Stretched to the pointer, in the bone's own units.
          const o = w[i].origin;
          const sx = Math.hypot(w[i].m[0], w[i].m[1]) || 1;
          patch.length = Math.max(1, Math.hypot(p[0] - o[0], p[1] - o[1]) / sx);
        }
        patchBone(name, patch);
      }
    }
  }

  function up() {
    if (drag?.kind === "draw" && ghost) {
      const a = ghost.from;
      const b = ghost.to;
      if (Math.hypot(b[0] - a[0], b[1] - a[1]) * view.scale > 6) addBoneBetween(a, b);
    }
    drag = null;
    ghost = null;
  }

  function paintAt(p: [number, number], invertMode: boolean) {
    const mesh = doc.mesh;
    const bone = selBone;
    if (!mesh || !bone) return;
    const hits = brushHits(mesh.vertices, p, brushRadius / view.scale);
    if (hits.length === 0) return;
    const m = invertMode ? (brushMode === "add" ? "remove" : brushMode === "remove" ? "add" : "smooth") : brushMode;
    const step = brushStrength * 0.35;
    const weights =
      m === "smooth"
        ? smoothWeights(mesh.weights, mesh.indices, hits, step * 2)
        : paintWeights(mesh.weights, hits, bone, m === "add" ? step : -step, (i) => heirOf(i, bone));
    doc = { ...doc, mesh: { ...mesh, weights } };
    dirty = true;
  }

  /** A new bone from `a` to `b` (root space), a child of the selected one. */
  function addBoneBetween(a: [number, number], b: [number, number]) {
    const parent = selBone ? skel.findIndex((x) => x.name === selBone) : -1;
    const frames = boneFrames(skel);
    const m: Affine = parent >= 0 ? frames.get(skel[parent].name)! : IDENTITY;
    const inv = invert(m);
    const pa = applyAffine(inv, a[0], a[1]);
    const pb = applyAffine(inv, b[0], b[1]);
    const name = freshBoneName({ name: "", bones: doc.bones });
    const bone: Bone = {
      name,
      entity: "",
      ...(parent >= 0 ? { parent: skel[parent].name } : {}),
      position: [pa[0], pa[1]],
      rotation: Math.atan2(pb[1] - pa[1], pb[0] - pa[0]),
      scale: [1, 1],
      length: Math.hypot(pb[0] - pa[0], pb[1] - pa[1]),
    };
    edit({ bones: [...doc.bones, bone] });
    selBone = name;
  }

  function removeSelected() {
    if (!selBone) return;
    const gone = doc.bones.find((b) => b.name === selBone);
    if (!gone) return;
    const heir = gone.parent ?? doc.bones.find((b) => b.parent === gone.name)?.name ?? null;
    const bones = removeBone(asRig(doc.bones), gone.name).bones as Bone[];
    edit({
      bones,
      removed: gone.entity ? [...doc.removed, gone.entity] : doc.removed,
      mesh: doc.mesh ? { ...doc.mesh, weights: handOver(doc.mesh.weights, gone.name, heir) } : null,
    });
    selBone = null;
  }

  function rename(from: string, to: string) {
    to = to.trim();
    const rig = asRig(doc.bones);
    const next = renameBone(rig, from, to);
    if (next === rig) return;
    edit({ bones: next.bones as Bone[], mesh: doc.mesh ? { ...doc.mesh, weights: renameWeights(doc.mesh.weights, from, to) } : null });
    selBone = to;
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    viewMoved = true;
    const before = toWorld(e.offsetX, e.offsetY);
    view = { ...view, scale: view.scale * Math.exp(-e.deltaY * 0.0015) };
    const after = toWorld(e.offsetX, e.offsetY);
    view = { ...view, cx: view.cx + before[0] - after[0], cy: view.cy + before[1] - after[1] };
  }

  // ── Apply ──

  let canApply = $derived(doc.bones.length > 0 && !!doc.mesh && !!doc.texture);

  function applyToScene() {
    const mesh = doc.mesh;
    if (!mesh || !doc.texture) return;
    history.flush();
    const bones = $state.snapshot(doc.bones) as Bone[];
    const removed = new Set(doc.removed);
    const component = toSkinnedMesh($state.snapshot(mesh) as SkinMesh, skel, doc.texture, {
      ...(existingSkin?.color ? { color: existingSkin.color } : {}),
      ...(existingSkin?.z_order !== undefined ? { z_order: existingSkin.z_order } : {}),
    });
    let made = rootName;
    execute(`Skin ${rootName || "a character"}`, (s) => {
      const names = new Set(s.entities.map((e) => e.name));
      const unique = (base: string) => {
        if (!names.has(base)) return (names.add(base), base);
        for (let n = 2; ; n++) if (!names.has(`${base}_${n}`)) return (names.add(`${base}_${n}`), `${base}_${n}`);
      };
      let root = s.entities.find((e) => e.name === rootName);
      if (!root) {
        const base = (doc.texture.split("/").pop() ?? "character").replace(/\.[^.]+$/, "");
        const name = unique(base);
        root = {
          name,
          components: { transform: { position: [0, 0], rotation: 0, scale: [1, 1] }, skeleton_root: { name } } as any,
        };
        s.entities.push(root);
      }
      made = root.name;
      // Bones removed here go, their other children moving up to them.
      for (const gone of removed) {
        const e = s.entities.find((x) => x.name === gone);
        if (!e) continue;
        for (const c of s.entities) if (c.parent === gone) c.parent = e.parent ?? null;
        s.entities = s.entities.filter((x) => x !== e);
      }
      // Each bone's entity: found, or made under its parent's.
      const entityOf = new Map<string, string>();
      for (const b of bones) if (b.entity && s.entities.some((e) => e.name === b.entity)) entityOf.set(b.name, b.entity);
      const ordered = skeletonFromRig({ name: "", bones }).map((sb) => bones.find((b) => b.name === sb.name)!);
      for (const b of ordered) {
        const parentEntity = b.parent ? entityOf.get(b.parent) ?? made : made;
        const transform = { position: b.position ?? [0, 0], rotation: b.rotation ?? 0, scale: b.scale ?? [1, 1] };
        // A bone that has an entity keeps where it hangs (a non-bone entity
        // may sit between it and its parent bone); a new one hangs from its
        // parent bone's.
        let e = s.entities.find((x) => x.name === entityOf.get(b.name));
        if (!e) {
          e = { name: unique(`${made}_${b.name}`), parent: parentEntity, components: {} as any };
          s.entities.push(e);
          entityOf.set(b.name, e.name);
        }
        const comps = e.components as any;
        comps.transform = { ...(comps.transform ?? {}), ...transform };
        comps.bone = { ...(comps.bone ?? {}), name: b.name, length: b.length ?? 1 };
      }
      (root.components as any).skinned_mesh = component;
      return s;
    });
    // The bones now have entities; the next Apply updates them.
    rootName = made;
    const after = get(scene);
    const index = after.entities.findIndex((e) => e.name === made);
    doc = { ...doc, bones: sceneRigBones(after.entities as any, index) as Bone[], removed: [] };
    history.reset();
    dirty = false;
    if (index >= 0) selectEntity(index);
    logInfo(`Skinned ${made}: ${component.vertices.length} vertices on ${component.bones.length} bones`);
  }

  function close() {
    if (dirty && !confirm("Close without applying the changes to the scene?")) return;
    onclose();
  }

  let overlay = $state<HTMLDivElement | null>(null);
  $effect(() => overlay?.focus());

  function onKeydown(e: KeyboardEvent) {
    const t = e.target as HTMLElement;
    if (t.tagName === "INPUT" || t.tagName === "SELECT" || t.tagName === "TEXTAREA") return;
    if ((e.key === "Delete" || e.key === "Backspace") && mode === "bones" && selBone) {
      e.preventDefault();
      removeSelected();
    } else if (e.key === "Escape") close();
    else if (e.key === "d" && mode === "bones") boneTool = "draw";
    else if (e.key === "e" && mode === "bones") boneTool = "edit";
    else if (e.key === "1") mode = "bones";
    else if (e.key === "2") mode = "mesh";
    else if (e.key === "3") mode = "weights";
    else if (e.key === "4") mode = "pose";
  }

  let selRigBone = $derived(doc.bones.find((b) => b.name === selBone) ?? null);
  const num = (e: Event) => Number((e.target as HTMLInputElement).value);
  const deg = (r: number) => Math.round(((r * 180) / Math.PI) * 10) / 10;
  let tree = $derived.by(() => {
    const out: { name: string; depth: number }[] = [];
    const visit = (parent: string | undefined, depth: number) => {
      for (const b of doc.bones) {
        if ((b.parent ?? undefined) === parent && !out.some((o) => o.name === b.name)) {
          out.push({ name: b.name, depth });
          visit(b.name, depth + 1);
        }
      }
    };
    visit(undefined, 0);
    for (const b of doc.bones) if (!out.some((o) => o.name === b.name)) out.push({ name: b.name, depth: 0 });
    return out;
  });
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="overlay" role="dialog" aria-label="Skin editor" tabindex="-1" bind:this={overlay} onkeydown={onKeydown}>
  <div class="window">
    <div class="head">
      <span class="title">Skin & rig: {rootName || "a new character"}{dirty ? " *" : ""}</span>
      <div class="tabs">
        {#each [["bones", "1 Bones"], ["mesh", "2 Mesh"], ["weights", "3 Weights"], ["pose", "4 Pose"]] as [m, label]}
          <button class:on={mode === m} onclick={() => (mode = m as Mode)}>{label}</button>
        {/each}
      </div>
      <span class="spacer"></span>
      <button onclick={() => ((viewMoved = false), frame())}>Frame</button>
      <button class="apply" disabled={!canApply} onclick={applyToScene} title="Write the bones and the skinned mesh into the scene (one undo step)">Apply to scene</button>
      <button onclick={close} title="Close">×</button>
    </div>
    <div class="body">
      <canvas
        bind:this={canvas}
        class="view"
        onpointerdown={down}
        onpointermove={move}
        onpointerup={up}
        onpointerleave={() => (hover = null)}
        onwheel={onWheel}
        oncontextmenu={(e) => e.preventDefault()}
      ></canvas>
      <div class="side">
        <label class="field">
          Picture
          <select value={doc.texture} onchange={(e) => edit({ texture: (e.target as HTMLSelectElement).value, place: null })}>
            <option value="">(pick one)</option>
            {#each pictures as p}<option value={p}>{p}</option>{/each}
          </select>
        </label>
        {#if doc.place}
          <label class="field">
            Pixel size
            <input type="number" min="0.01" step="0.1" disabled={!!doc.mesh} title={doc.mesh ? "Fixed once there is a mesh (its vertices sit on the picture)" : "World units a pixel"} value={doc.place.scale} onchange={(e) => { const s = Math.max(num(e), 0.01); edit({ place: centredPlacement(imgW, imgH, s) }); }} />
          </label>
        {/if}

        {#if mode === "bones"}
          <div class="modes">
            <button class:on={boneTool === "draw"} onclick={() => (boneTool = "draw")} title="D">Draw</button>
            <button class:on={boneTool === "edit"} onclick={() => (boneTool = "edit")} title="E">Edit</button>
          </div>
          {#if boneTool === "draw"}
            <div class="hint">Drag to draw a bone, a child of the selected one. Start at a bone's tip to carry on from it - shoulder, elbow, wrist.</div>
          {:else}
            <div class="hint">Drag a bone to turn it, its tip to stretch it, Shift-drag to move its joint. Delete removes the selected bone.</div>
          {/if}
          <div class="tree">
            {#each tree as b}
              <button class="row" class:sel={b.name === selBone} style="padding-left:{6 + b.depth * 12}px" onclick={() => (selBone = selBone === b.name ? null : b.name)}>{b.name}</button>
            {/each}
            {#if tree.length === 0}<div class="hint">No bones yet.</div>{/if}
          </div>
          {#if selRigBone}
            {@const b = selRigBone}
            <label class="field">Name <input value={b.name} onchange={(e) => rename(b.name, (e.target as HTMLInputElement).value)} /></label>
            <label class="field">Length <input type="number" min="1" value={Math.round((b.length ?? 1) * 10) / 10} onchange={(e) => patchBone(b.name, { length: Math.max(num(e), 1) })} /></label>
            <div class="hint">Turned {deg(b.rotation ?? 0)}° from its parent{b.entity ? `; entity ${b.entity}` : "; new"}.</div>
            <button onclick={removeSelected}>Remove bone</button>
          {/if}
        {:else if mode === "mesh"}
          <div class="hint">Covers the picture's opaque pixels with a grid of triangles; the picture's own edges still draw its outline. Smaller cells bend more smoothly.</div>
          <label class="field">Cell (px) <input type="range" min="3" max="64" bind:value={cellSize} /> {cellSize}</label>
          <label class="field">Alpha over <input type="range" min="0" max="250" bind:value={alphaCut} /> {alphaCut}</label>
          <button disabled={!img} onclick={generateMesh}>{doc.mesh ? "Rebuild the mesh" : "Build the mesh"}</button>
          {#if doc.mesh}
            <div class="hint">{doc.mesh.vertices.length} vertices, {doc.mesh.indices.length / 3} triangles. Building weighs it from the bones.</div>
          {/if}
        {:else if mode === "weights"}
          <div class="hint">Paint the selected bone's weight: red is all of it, blue none. Shift inverts the brush; Ctrl-click a bone to pick it.</div>
          <div class="tree">
            {#each tree as b}
              <button class="row" class:sel={b.name === selBone} style="padding-left:{6 + b.depth * 12}px" onclick={() => (selBone = b.name)}>{b.name}</button>
            {/each}
          </div>
          <div class="modes">
            {#each [["add", "Add"], ["remove", "Remove"], ["smooth", "Smooth"]] as [m, label]}
              <button class:on={brushMode === m} onclick={() => (brushMode = m as typeof brushMode)}>{label}</button>
            {/each}
          </div>
          <label class="field">Radius <input type="range" min="5" max="200" bind:value={brushRadius} /> {brushRadius}px</label>
          <label class="field">Strength <input type="range" min="0.05" max="1" step="0.05" bind:value={brushStrength} /> {brushStrength}</label>
          <label class="field">Bones a vertex <input type="range" min="1" max="4" bind:value={influences} /> {influences}</label>
          <button disabled={!doc.mesh || doc.bones.length === 0} onclick={weighAll}>Weigh again from the bones</button>
          {#if !doc.mesh}<div class="warn">Build the mesh first (2 Mesh).</div>{/if}
        {:else}
          <div class="hint">Drag bones to see the skin bend. Nothing here is saved.</div>
          <button onclick={() => (pose = {})} disabled={Object.keys(pose).length === 0}>Back to the bind pose</button>
        {/if}
        {#if !canApply}
          <div class="warn">
            To apply: {[!doc.texture && "a picture", doc.bones.length === 0 && "bones", !doc.mesh && "a mesh"].filter(Boolean).join(", ")}.
          </div>
        {/if}
      </div>
    </div>
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
    width: min(1200px, 96vw);
    height: min(820px, 92vh);
    display: flex;
    flex-direction: column;
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    border-radius: 6px;
    color: var(--ed-text);
    font-size: 12px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--ed-line);
  }
  .title {
    font-weight: 600;
  }
  .tabs,
  .modes {
    display: flex;
    gap: 2px;
  }
  .tabs button.on,
  .modes button.on {
    background: var(--ed-line);
    border-color: var(--ed-accent);
  }
  .spacer {
    flex: 1;
  }
  .body {
    flex: 1;
    min-height: 0;
    display: flex;
  }
  .view {
    flex: 1;
    min-width: 0;
    width: 100%;
    height: 100%;
    touch-action: none;
    cursor: crosshair;
  }
  .side {
    width: 270px;
    border-left: 1px solid var(--ed-line);
    padding: 8px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    overflow-y: auto;
  }
  .field {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--ed-muted);
  }
  .field select,
  .field input:not([type="range"]) {
    flex: 1;
    min-width: 0;
  }
  .field input[type="range"] {
    flex: 1;
  }
  .tree {
    display: flex;
    flex-direction: column;
    border: 1px solid var(--ed-line);
    max-height: 220px;
    overflow-y: auto;
  }
  .row {
    text-align: left;
    border: none;
    border-radius: 0;
  }
  .row.sel {
    background: var(--ed-line);
  }
  .hint {
    color: var(--ed-faint);
  }
  .warn {
    color: var(--ed-warn);
  }
  input,
  select,
  button {
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    border-radius: 4px;
    font-size: 11.5px;
    padding: 3px 6px;
  }
  button {
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .apply:not(:disabled) {
    border-color: var(--ed-ok);
  }
</style>
