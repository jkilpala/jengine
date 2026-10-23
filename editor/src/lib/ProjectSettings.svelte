<script lang="ts">
  import { SettledHistory } from "./settledHistory";
  import { claimUndo } from "./stores/undoTargetStore";
  import { get } from "svelte/store";
  import { project } from "./stores/projectStore";
  import { logInfo, logError } from "./stores/consoleStore";
  import {
    saveProject,
    defaultSettings,
    type ProjectSettings,
    type InputAction,
    type AssetPipelineSettings,
  } from "./api/project";
  import { getAvailablePackages, type AvailablePackage } from "./api/packages";
  import { getEditorPrefs, setEditorPrefs, WASM_OPT_LEVELS, type EditorPrefs } from "./api/editorPrefs";
  import { open } from "@tauri-apps/plugin-dialog";
  import { SvelteSet } from "svelte/reactivity";
  import {
    ANDROID_ABIS,
    DEFAULT_MIN_SDK,
    DEFAULT_TARGET_SDK,
    DEFAULT_VERSION,
    cleanSettings,
    defaultAppId,
    defaultOrientation,
    validateAppId,
  } from "./mobileApp";

  // Binding names (keys, pad buttons and axes) come from `inputNames.json`,
  // which `input_names_match_the_editor` checks against the engine.
  import { KEY_NAMES, bindingChoices, isBinding, keyBinding, padBinding, type PadState } from "./inputBindings";
  import { cleanPacks, includeText, newPackName, packProblem, parseIncludes } from "./assetPacks";
  import { cleanLiveOps, liveOpsProblem, newKey, parseValue, renameKey, valueText } from "./liveOps";

  let { onclose }: { onclose: () => void } = $props();

  /**
   * Fill in fields a project saved before they existed.
   *
   * Applied to the plain object *before* it becomes state, rather than as
   * top-level statements afterwards. Reading `$state` at the top level of a
   * component captures only its initial value - harmless here, since this is
   * initialisation, but Svelte cannot tell those apart and warned twelve
   * times, which is twelve places nobody reads any more.
   */
  function withDefaults(s: ProjectSettings): ProjectSettings {
    const d = defaultSettings();
    if (!s.input_actions) s.input_actions = [];
    if (s.physics.fixed_timestep == null) s.physics.fixed_timestep = 1 / 60;
    if (s.physics.length_unit == null) s.physics.length_unit = 1.0;
    if (s.code_editor == null) s.code_editor = "";
    if (!s.asset_pipeline) s.asset_pipeline = d.asset_pipeline;
    if (!s.enabled_packages) s.enabled_packages = d.enabled_packages;
    // Bound field by field on the Mobile App page; empty ones are dropped
    // again on save.
    if (!s.app) s.app = {};
    if (!s.android) s.android = {};
    return s;
  }

  // Initialize from current project or defaults
  let settings: ProjectSettings = $state(
    withDefaults(
      get(project)?.settings
        ? structuredClone(get(project)!.settings)
        : defaultSettings(),
    ),
  );

  let projectName = $state(get(project)?.name ?? "");

  // Undo over the settings, a step each time they settle after a change
  // (`settledHistory.ts`); Ctrl+Z / Ctrl+Y are this dialog's while it is open.
  const history = new SettledHistory<ProjectSettings>(
    () => $state.snapshot(settings) as ProjectSettings,
    (v) => {
      settings = v;
    },
  );
  $effect(() => {
    JSON.stringify(settings);
    history.observe();
  });
  $effect(() => claimUndo({ undo: () => history.undo(), redo: () => history.redo() }));

  // Hex conversion helpers
  function colorToHex(c: [number, number, number, number]): string {
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

  function hexToColor(hex: string): [number, number, number] {
    const r = parseInt(hex.slice(1, 3), 16) / 255;
    const g = parseInt(hex.slice(3, 5), 16) / 255;
    const b = parseInt(hex.slice(5, 7), 16) / 255;
    return [
      Math.round(r * 100) / 100,
      Math.round(g * 100) / 100,
      Math.round(b * 100) / 100,
    ];
  }

  async function handleSave() {
    const proj = get(project);
    if (!proj) return;
    try {
      const settingsSnapshot = $state.snapshot(settings) as ProjectSettings;
      // A cleared box binds "" or null; Rust wants the key absent instead.
      settingsSnapshot.app = cleanSettings(settingsSnapshot.app);
      settingsSnapshot.android = cleanSettings(settingsSnapshot.android);
      settingsSnapshot.asset_packs = cleanPacks(settingsSnapshot.asset_packs);
      settingsSnapshot.liveops = cleanLiveOps(settingsSnapshot.liveops);
      // The legacy top-level `default_scene` follows this field rather than
      // being carried through untouched. Both the editor and the runtime fall
      // back to it when `entry_scene` is unset, so leaving it alone meant
      // *clearing* the entry scene did nothing at all: the old value silently
      // took over from a field nothing in the UI shows.
      const entryScene = settingsSnapshot.entry_scene;
      await saveProject(
        proj.path,
        projectName,
        entryScene,
        settingsSnapshot,
        proj.component_definitions ?? [],
      );
      // Update the store
      project.update((p) => {
        if (!p) return p;
        return {
          ...p,
          name: projectName,
          settings: settingsSnapshot,
          default_scene: entryScene,
        };
      });
      logInfo("Project settings saved");
      onclose();
    } catch (e) {
      logError(`Failed to save project settings: ${e}`);
    }
  }

  function handleBgColorPick(hex: string) {
    const [r, g, b] = hexToColor(hex);
    settings.background_color = [r, g, b, settings.background_color[3]];
  }

  // The editor's own GPU (Graphics tab): an editor preference, not a project
  // setting, so it is saved as soon as it changes and applies from the next
  // start - WebView2 reads its browser arguments once.
  let editorPrefs = $state<EditorPrefs | null>(null);
  let editorGpuChanged = $state(false);
  $effect(() => {
    getEditorPrefs()
      .then((p) => (editorPrefs = p))
      .catch(() => {});
  });

  async function setEditorGpu(gpu: EditorPrefs["gpu"]) {
    if (!editorPrefs || editorPrefs.gpu === gpu) return;
    const next = { ...editorPrefs, gpu };
    try {
      await setEditorPrefs(next);
      editorPrefs = next;
      editorGpuChanged = true;
      logInfo(`Editor GPU set to ${gpu === "high_performance" ? "high performance" : "Windows' choice"} - restart the editor to apply.`);
    } catch (e) {
      logError(`Failed to save the editor GPU preference: ${e}`);
    }
  }

  // How hard the editor viewport is optimised when it is built: also an
  // editor preference, read by Rebuild viewport, `jengine viewport-build`
  // and run-editor - so it applies from the next viewport build.
  async function setViewportWasmOpt(level: EditorPrefs["viewport_wasm_opt"]) {
    if (!editorPrefs || editorPrefs.viewport_wasm_opt === level) return;
    const next = { ...editorPrefs, viewport_wasm_opt: level };
    try {
      await setEditorPrefs(next);
      editorPrefs = next;
      logInfo(`Editor viewport builds will use wasm-opt "${level}" from the next build.`);
    } catch (e) {
      logError(`Failed to save the viewport optimisation preference: ${e}`);
    }
  }

  // Load available built-in packages for the Packages tab
  let availablePackages = $state<AvailablePackage[]>([]);
  $effect(() => {
    getAvailablePackages()
      .then((pkgs) => (availablePackages = pkgs))
      .catch(() => {});
  });

  function togglePackage(name: string) {
    const idx = settings.enabled_packages.indexOf(name);
    if (idx >= 0) {
      settings.enabled_packages = settings.enabled_packages.filter(
        (n) => n !== name,
      );
    } else {
      settings.enabled_packages = [...settings.enabled_packages, name];
    }
  }

  type Category =
    | "general"
    | "display"
    | "graphics"
    | "mobile"
    | "physics"
    | "input"
    | "assets"
    | "liveops"
    | "web"
    | "packages";
  let activeCategory = $state<Category>("general");

  const categories: { id: Category; label: string; icon: string }[] = [
    { id: "general", label: "General", icon: "⚙" },
    { id: "display", label: "Display", icon: "🖵" },
    { id: "graphics", label: "Graphics", icon: "🎨" },
    { id: "mobile", label: "Mobile App", icon: "📱" },
    { id: "physics", label: "Physics", icon: "🔧" },
    { id: "input", label: "Input Actions", icon: "⌨" },
    { id: "assets", label: "Asset Pipeline", icon: "📦" },
    { id: "liveops", label: "Live Ops", icon: "📡" },
    { id: "web", label: "Web", icon: "🌐" },
    { id: "packages", label: "Packages", icon: "🧩" },
  ];

  /** Platforms a project can pin backends for, with what wgpu supports there. */
  const BACKEND_CHOICES: {
    platform: "web" | "windows" | "macos" | "ios" | "android";
    label: string;
    options: { id: string; label: string; note?: string }[];
    note?: string;
  }[] = [
    {
      platform: "web",
      label: "Web",
      options: [
        { id: "webgpu", label: "WebGPU", note: "smallest build" },
        {
          id: "webgl",
          label: "WebGL2",
          note: "fallback for browsers without WebGPU; adds ~size",
        },
      ],
    },
    {
      platform: "windows",
      label: "Windows",
      options: [
        { id: "dx12", label: "DirectX 12" },
        { id: "vulkan", label: "Vulkan", note: "alternative for odd drivers" },
      ],
    },
    {
      platform: "macos",
      label: "macOS",
      options: [{ id: "metal", label: "Metal" }],
      note: "No build target yet — stored for when one lands.",
    },
    {
      platform: "ios",
      label: "iOS",
      options: [{ id: "metal", label: "Metal" }],
      note: "No build target yet — stored for when one lands.",
    },
    {
      platform: "android",
      label: "Android",
      options: [{ id: "vulkan", label: "Vulkan" }],
      note: "Every Android build also carries GLES, the fallback for phones without usable Vulkan.",
    },
  ];

  const DEFAULT_BACKENDS: Record<string, string[]> = {
    web: ["webgpu"],
    windows: ["dx12"],
    macos: ["metal"],
    ios: ["metal"],
    android: ["vulkan"],
  };

  function backendsFor(platform: string): string[] {
    const gb = settings.graphics_backends as
      | Record<string, string[]>
      | undefined;
    return gb?.[platform] ?? DEFAULT_BACKENDS[platform] ?? [];
  }

  function toggleBackend(platform: string, id: string, on: boolean) {
    if (!settings.graphics_backends) {
      settings.graphics_backends = {
        web: [...DEFAULT_BACKENDS.web],
        windows: [...DEFAULT_BACKENDS.windows],
        macos: [...DEFAULT_BACKENDS.macos],
        ios: [...DEFAULT_BACKENDS.ios],
        android: [...DEFAULT_BACKENDS.android],
      };
    }
    const gb = settings.graphics_backends as unknown as Record<string, string[]>;
    const current = new Set(gb[platform] ?? []);
    if (on) current.add(id);
    else current.delete(id);
    // Order matters: wgpu tries backends in the order they are compiled in,
    // so keep the declared option order rather than click order.
    const ordered = BACKEND_CHOICES.find(
      (c) => c.platform === platform,
    )!.options.filter((o) => current.has(o.id));
    gb[platform] = ordered.map((o) => o.id);
    settings = settings;
  }

  // `settings.asset_packs`: parts of the game a build moves out to download.
  function addPack() {
    const ap = (settings.asset_packs ??= {});
    ap.packs = [...(ap.packs ?? []), { name: newPackName(ap), include: [] }];
  }

  function removePack(index: number) {
    const ap = settings.asset_packs;
    if (ap?.packs) ap.packs = ap.packs.filter((_, i) => i !== index);
  }

  // `settings.liveops`: the service a built game asks, and its defaults.
  function liveopsField(field: "url" | "channel" | "editor_channel" | "public_key", value: string) {
    (settings.liveops ??= {})[field] = value.trim();
  }

  function addDefault() {
    const lo = (settings.liveops ??= {});
    lo.defaults = { ...(lo.defaults ?? {}), [newKey(lo)]: "" };
  }

  function removeDefault(key: string) {
    const lo = settings.liveops;
    if (!lo?.defaults) return;
    const { [key]: _gone, ...rest } = lo.defaults;
    lo.defaults = rest;
  }

  function setDefault(key: string, text: string) {
    const lo = (settings.liveops ??= {});
    lo.defaults = { ...(lo.defaults ?? {}), [key]: parseValue(text) };
  }

  function renameDefault(from: string, to: string) {
    const lo = settings.liveops;
    if (lo?.defaults) lo.defaults = renameKey(lo.defaults, from, to.trim());
  }

  // Which actions are unfolded, by index. Folded is the default: a folded
  // action still shows its bindings, and a project has dozens of actions.
  const openActions = new SvelteSet<number>();

  function toggleAction(index: number) {
    if (openActions.has(index)) openActions.delete(index);
    else openActions.add(index);
  }

  function addAction() {
    settings.input_actions = [
      ...settings.input_actions,
      { name: "", keys: [] },
    ];
    openActions.add(settings.input_actions.length - 1);
  }

  function removeAction(index: number) {
    settings.input_actions = settings.input_actions.filter(
      (_, i) => i !== index,
    );
    // The actions after it move up one, and their folds with them.
    const open = [...openActions];
    openActions.clear();
    for (const i of open) {
      if (i < index) openActions.add(i);
      else if (i > index) openActions.add(i - 1);
    }
  }

  // ── Frame rate cap ──────────────────────────────────────────────────
  const FRAME_CAP_PLATFORMS: {
    id: "desktop" | "web" | "android";
    label: string;
    fallback: string;
  }[] = [
    { id: "desktop", label: "Desktop", fallback: "display rate" },
    { id: "web", label: "Web", fallback: "display rate" },
    { id: "android", label: "Android", fallback: "60 fps" },
  ];

  /// "" is the platform default (the key left out); "0" is display rate.
  // ── Texture filtering ───────────────────────────────────────────────
  // `settings.texture_filter`: a default and texture-id prefix overrides.
  // Kept as an object on disk; edited here as rows, in their saved order.
  type FilterMode = "nearest" | "linear";

  function setTextureFilterDefault(mode: FilterMode) {
    settings.texture_filter = { ...(settings.texture_filter ?? {}), default: mode };
    pruneTextureFilter();
  }

  function filterOverrides(): [string, FilterMode][] {
    return Object.entries(settings.texture_filter?.overrides ?? {}) as [string, FilterMode][];
  }

  function writeOverrides(rows: [string, FilterMode][]) {
    const overrides: Record<string, FilterMode> = {};
    for (const [prefix, mode] of rows) overrides[prefix] = mode;
    settings.texture_filter = { ...(settings.texture_filter ?? {}), overrides };
    pruneTextureFilter();
  }

  function editOverride(i: number, prefix: string, mode: FilterMode) {
    const rows = filterOverrides();
    rows[i] = [prefix, mode];
    writeOverrides(rows);
  }

  function addOverride() {
    const rows = filterOverrides();
    const fallback: FilterMode = (settings.texture_filter?.default ?? "nearest") === "nearest" ? "linear" : "nearest";
    rows.push(["sprites/", fallback]);
    writeOverrides(rows);
  }

  function removeOverride(i: number) {
    const rows = filterOverrides();
    rows.splice(i, 1);
    writeOverrides(rows);
  }

  /** Nothing set is the same as all-nearest: leave the key off the file. */
  function pruneTextureFilter() {
    const tf = settings.texture_filter;
    if (!tf) return;
    if (tf.overrides && Object.keys(tf.overrides).length === 0) delete tf.overrides;
    if ((tf.default ?? "nearest") === "nearest" && !tf.overrides) settings.texture_filter = undefined;
  }

  function setFrameCap(platform: "desktop" | "web" | "android", value: string) {
    const caps = { ...(settings.frame_rate_cap ?? {}) };
    if (value === "") delete caps[platform];
    else caps[platform] = Number(value);
    settings.frame_rate_cap = Object.keys(caps).length ? caps : undefined;
  }

  // ── Mobile App ──────────────────────────────────────────────────────
  const appIdError = $derived(validateAppId(settings.app?.id ?? ""));

  /** Pick the icon, stored relative to the project when it is inside it. */
  async function browseIcon() {
    const root = (get(project)?.path ?? "").replace(/\\/g, "/");
    const picked = await open({
      multiple: false,
      defaultPath: root || undefined,
      filters: [{ name: "PNG image", extensions: ["png"] }],
    });
    if (typeof picked !== "string") return;
    const path = picked.replace(/\\/g, "/");
    const prefix = root.endsWith("/") ? root : root + "/";
    settings.app!.icon =
      root && path.toLowerCase().startsWith(prefix.toLowerCase())
        ? path.slice(prefix.length)
        : path;
  }

  function toggleAbi(abi: string, on: boolean) {
    const current = settings.android!.abis?.length
      ? settings.android!.abis
      : ["arm64-v8a"];
    settings.android!.abis = on
      ? [...new Set([...current, abi])]
      : current.filter((a) => a !== abi);
  }

  function addKeyToAction(actionIndex: number) {
    const action = settings.input_actions[actionIndex];
    action.keys = [...action.keys, KEY_NAMES[0]];
    settings.input_actions = [...settings.input_actions];
  }

  /** The binding being captured by "press to bind": a key or a pad press
   *  becomes it; Escape cancels. */
  let capturing = $state<{ action: number; key: number } | null>(null);

  function setBinding(actionIndex: number, keyIndex: number, name: string) {
    const action = settings.input_actions[actionIndex];
    action.keys[keyIndex] = name;
    settings.input_actions = [...settings.input_actions];
  }

  function padStates(): PadState[] {
    const pads = typeof navigator !== "undefined" && navigator.getGamepads ? navigator.getGamepads() : [];
    return [...pads].map((g) => ({
      buttons: g ? g.buttons.map((b) => b.value) : [],
      axes: g ? [...g.axes] : [],
    }));
  }

  function startCapture(actionIndex: number, keyIndex: number) {
    capturing = { action: actionIndex, key: keyIndex };
    let before = padStates();
    const finish = (name: string | null) => {
      window.removeEventListener("keydown", onKey, true);
      cancelAnimationFrame(frame);
      if (name && capturing) setBinding(capturing.action, capturing.key, name);
      capturing = null;
    };
    // Keys reach this before the app's own shortcuts.
    const onKey = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.code === "Escape" && !e.shiftKey) {
        finish(null);
        return;
      }
      const name = keyBinding(e.code);
      if (name) finish(name);
    };
    window.addEventListener("keydown", onKey, true);
    let frame = requestAnimationFrame(function poll() {
      const now = padStates();
      for (let p = 0; p < now.length; p++) {
        const name = padBinding(before[p] ?? { buttons: [], axes: [] }, now[p], p);
        if (name) {
          finish(name);
          return;
        }
      }
      before = now;
      frame = requestAnimationFrame(poll);
    });
  }

  function hasAxis(keys: string[]): boolean {
    return keys.some((k) => /Stick[XY][+-]$|Trigger\+?$/.test(k));
  }

  function removeKeyFromAction(actionIndex: number, keyIndex: number) {
    const action = settings.input_actions[actionIndex];
    action.keys = action.keys.filter((_, i) => i !== keyIndex);
    settings.input_actions = [...settings.input_actions];
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="overlay">
  <div class="dialog" onclick={(e) => e.stopPropagation()}>
    <div class="dialog-header">
      <span>Project Settings</span>
      <button class="close-btn" onclick={onclose}>×</button>
    </div>

    <div class="dialog-body">
      <div class="category-nav" role="tablist">
        {#each categories as cat}
          <button
            role="tab"
            class="category-btn"
            class:active={activeCategory === cat.id}
            aria-selected={activeCategory === cat.id}
            onclick={() => (activeCategory = cat.id)}
          >
            <span class="cat-icon">{cat.icon}</span>
            <span class="cat-label">{cat.label}</span>
          </button>
        {/each}
      </div>

      <div class="category-content" role="tabpanel">
        {#if activeCategory === "general"}
          <h3 class="content-title">General</h3>
          <div class="field-row">
            <label>Project Name</label>
            <input type="text" bind:value={projectName} />
          </div>
          <div class="field-row">
            <label>Window Title</label>
            <input type="text" bind:value={settings.title} />
          </div>
          <div class="field-row">
            <label>Entry Scene</label>
            <select
              value={settings.entry_scene ?? ""}
              onchange={(e) => {
                const v = (e.target as HTMLSelectElement).value;
                settings.entry_scene = v || null;
              }}
            >
              <option value="">-- None --</option>
              {#each get(project)?.scene_files ?? [] as scene}
                <option value={scene}>{scene}</option>
              {/each}
            </select>
            <span class="field-hint">Scene loaded when the game starts</span>
          </div>
          <div class="field-row">
            <label>Code Editor</label>
            <input
              type="text"
              bind:value={settings.code_editor}
              placeholder="auto (code, cursor, codium)"
            />
          </div>
        {:else if activeCategory === "display"}
          <h3 class="content-title">Display</h3>
          <div class="field-row">
            <label>Width</label>
            <input
              type="number"
              min="320"
              max="3840"
              bind:value={settings.width}
            />
          </div>
          <div class="field-row">
            <label>Height</label>
            <input
              type="number"
              min="240"
              max="2160"
              bind:value={settings.height}
            />
          </div>
          <div class="field-row">
            <label>UI scaling</label>
            <select bind:value={settings.ui_scale_mode}>
              <option value={undefined}>Scale with screen (default)</option>
              <option value="ScaleAndCenter">Scale and center</option>
              <option value="ConstantPixelSize">Constant pixel size</option>
            </select>
          </div>
          <p class="field-hint">
            Scale with screen draws both UI systems at
            <code>min(w / {settings.width}, h / {settings.height})</code>, so a UI
            authored at this size fits any window; on a wider window the spare
            width is the UI's, so edge-anchored elements reach the edges. Scale
            and center lays the UI out at exactly this size and centres it, so
            a UI placed in absolute positions stays centred on a phone. Constant
            pixel size is one UI pixel per screen pixel &mdash; pick it if your
            scripts already fit their own UI.
          </p>
          <div class="field-row" style="align-items: flex-start;">
            <label>Frame Rate Cap</label>
            <div style="display:flex; flex-direction:column; gap:4px;">
              {#each FRAME_CAP_PLATFORMS as p}
                <label class="check-option">
                  <span style="min-width: 60px;">{p.label}</span>
                  <select
                    value={settings.frame_rate_cap?.[p.id] ?? ""}
                    onchange={(e) => setFrameCap(p.id, (e.target as HTMLSelectElement).value)}
                  >
                    <option value="">Default ({p.fallback})</option>
                    <option value="0">Display rate</option>
                    {#each [30, 60, 90, 120] as fps}
                      <option value={String(fps)}>{fps} fps</option>
                    {/each}
                  </select>
                </label>
              {/each}
            </div>
          </div>
          <p class="field-hint">
            The most frames a second a build draws, kept evenly spaced (on a
            165 Hz monitor a 60 cap is 55). Phones default to 60 for battery and
            heat. The editor's Play mode is never capped.
          </p>
          <div class="field-row">
            <label>Background</label>
            <input
              type="color"
              value={colorToHex(settings.background_color)}
              onchange={(e) =>
                handleBgColorPick((e.target as HTMLInputElement).value)}
            />
            <label class="short-label">A</label>
            <input
              type="number"
              step="0.05"
              min="0"
              max="1"
              value={settings.background_color[3]}
              onchange={(e) => {
                settings.background_color[3] =
                  parseFloat((e.target as HTMLInputElement).value) || 1;
              }}
            />
          </div>
        {:else if activeCategory === "graphics"}
          <h3 class="content-title">Graphics Backends</h3>
          <p class="field-hint" style="margin-bottom: 12px;">
            wgpu compiles a separate code path per graphics API and picks one
            at runtime. Building only what a platform can use keeps the binary
            smaller — a web build with WebGPU alone is the smallest option.
            Leaving a platform with nothing ticked falls back to its default.
          </p>
          {#each BACKEND_CHOICES as group}
            <div class="field-row" style="align-items: flex-start;">
              <label>{group.label}</label>
              <div style="display:flex; flex-direction:column; gap:4px;">
                {#each group.options as option}
                  <label class="check-option">
                    <input
                      type="checkbox"
                      checked={backendsFor(group.platform).includes(option.id)}
                      onchange={(e) =>
                        toggleBackend(
                          group.platform,
                          option.id,
                          (e.target as HTMLInputElement).checked,
                        )}
                    />
                    <span>{option.label}</span>
                    {#if option.note}
                      <span class="field-hint">— {option.note}</span>
                    {/if}
                  </label>
                {/each}
                {#if group.note}
                  <span class="field-hint">{group.note}</span>
                {/if}
              </div>
            </div>
          {/each}
          <h3 class="content-title" style="margin-top: 20px;">GPU</h3>
          <div class="field-row">
            <label>Built game</label>
            <select
              value={settings.gpu_preference ?? "high_performance"}
              onchange={(e) => {
                const v = (e.target as HTMLSelectElement).value;
                settings.gpu_preference = v === "low_power" ? "low_power" : undefined;
              }}
            >
              <option value="high_performance">High performance (discrete GPU)</option>
              <option value="low_power">Low power (integrated GPU)</option>
            </select>
          </div>
          <p class="field-hint">
            Which GPU the game asks for on a machine with two, such as a gaming
            laptop. Low power saves battery and suits a light game. Native builds
            get what they ask for; a browser may ignore the request.
          </p>
          <div class="field-row">
            <label>Editor</label>
            <select
              value={editorPrefs?.gpu ?? "high_performance"}
              disabled={!editorPrefs}
              onchange={(e) => setEditorGpu((e.target as HTMLSelectElement).value as EditorPrefs["gpu"])}
            >
              <option value="high_performance">High performance (discrete GPU)</option>
              <option value="system">Let Windows choose</option>
            </select>
          </div>
          <p class="field-hint">
            The editor's own GPU, for every project - not saved with this one.
            Applies from the next editor start{editorGpuChanged ? " (restart to apply)" : ""}.
            "Let Windows choose" follows Settings › Display › Graphics, usually the
            integrated GPU.
          </p>
          <div class="field-row">
            <label>Viewport build</label>
            <select
              value={editorPrefs?.viewport_wasm_opt ?? "optimized"}
              disabled={!editorPrefs}
              onchange={(e) => setViewportWasmOpt((e.target as HTMLSelectElement).value as EditorPrefs["viewport_wasm_opt"])}
            >
              {#each WASM_OPT_LEVELS as level (level.id)}
                <option value={level.id}>{level.label}</option>
              {/each}
            </select>
          </div>
          <p class="field-hint">
            How hard the editor viewport's WebAssembly is optimised when it is
            rebuilt (a project's Rust packages, or the engine's own Rust), for every
            project. Fast builds in seconds what Optimized takes minutes over, for a
            slightly larger, slightly slower module - fine while editing; judge frame
            rates on Optimized. A game's web build has its own level, in Build
            Settings.
          </p>
          <h3 class="content-title" style="margin-top: 20px;">Texture Filtering</h3>
          <div class="field-row">
            <label>Default</label>
            <select
              value={settings.texture_filter?.default ?? "nearest"}
              onchange={(e) => setTextureFilterDefault((e.target as HTMLSelectElement).value as FilterMode)}
            >
              <option value="nearest">Nearest (pixel art)</option>
              <option value="linear">Linear, with mipmaps</option>
            </select>
          </div>
          {#each filterOverrides() as [prefix, mode], i}
            <div class="field-row">
              <label>{i === 0 ? "Overrides" : ""}</label>
              <div style="display:flex; gap:6px; align-items:center;">
                <input
                  type="text"
                  value={prefix}
                  placeholder="sprites/pixel/"
                  onchange={(e) => editOverride(i, (e.target as HTMLInputElement).value, mode)}
                />
                <select
                  value={mode}
                  onchange={(e) => editOverride(i, prefix, (e.target as HTMLSelectElement).value as FilterMode)}
                >
                  <option value="nearest">Nearest</option>
                  <option value="linear">Linear</option>
                </select>
                <button type="button" class="field-btn" onclick={() => removeOverride(i)} title="Remove">&times;</button>
              </div>
            </div>
          {/each}
          <div class="field-row">
            <label></label>
            <button type="button" class="field-btn" onclick={addOverride}>Add override</button>
          </div>
          <p class="field-hint">
            Nearest draws every texel as a hard-edged square &mdash; pixel art.
            Linear blends between texels and gives the texture mipmaps, so a
            texture drawn larger stays smooth and one drawn smaller does not
            shimmer. An override applies to every texture whose path starts
            with its prefix (the longest match wins). A packed sheet filtered
            linearly bleeds its neighbours into each region's edge: pad it, or
            keep it nearest.
          </p>
        {:else if activeCategory === "mobile"}
          <h3 class="content-title">Mobile App</h3>
          <p class="field-hint" style="margin-bottom: 12px;">
            How the game installs on a phone. An empty field uses the value
            shown greyed out. Android builds read these now; iOS builds will.
          </p>
          <div class="field-row">
            <label>App ID</label>
            <input
              type="text"
              bind:value={settings.app!.id}
              placeholder={defaultAppId(projectName || "game")}
              spellcheck="false"
            />
          </div>
          {#if appIdError}
            <p class="field-hint field-error">App ID {appIdError}</p>
          {:else}
            <p class="field-hint">
              The Android package name and iOS bundle id. A store knows the app
              by it, so it cannot change after the first release.
            </p>
          {/if}
          <div class="field-row">
            <label>Display Name</label>
            <input
              type="text"
              bind:value={settings.app!.name}
              placeholder={settings.title || projectName}
            />
            <span class="field-hint">Under the icon</span>
          </div>
          <div class="field-row">
            <label>Version</label>
            <input
              type="text"
              bind:value={settings.app!.version}
              placeholder={DEFAULT_VERSION}
            />
            <label class="short-label">Build</label>
            <input
              type="number"
              min="1"
              step="1"
              bind:value={settings.app!.build}
              placeholder="1"
            />
          </div>
          <p class="field-hint">
            Players see the version. Stores compare the build number, and every
            upload needs a higher one than the last.
          </p>
          <div class="field-row">
            <label>Icon</label>
            <input
              type="text"
              bind:value={settings.app!.icon}
              placeholder="icon.png (square PNG, 512 px or more)"
              spellcheck="false"
            />
            <button type="button" class="field-btn" onclick={browseIcon}>Browse…</button>
          </div>
          <div class="field-row">
            <label>Icon Background</label>
            <input
              type="color"
              value={settings.app!.icon_background || "#ffffff"}
              onchange={(e) =>
                (settings.app!.icon_background = (e.target as HTMLInputElement).value)}
            />
            <span class="field-hint">
              Behind the icon, where a launcher crops it to its own shape
            </span>
          </div>
          <div class="field-row">
            <label>Orientation</label>
            <select bind:value={settings.app!.orientation}>
              <option value={undefined}>
                Automatic ({defaultOrientation(settings.width, settings.height)}, from
                {settings.width} x {settings.height})
              </option>
              <option value="landscape">Landscape</option>
              <option value="portrait">Portrait</option>
              <option value="any">Follow the phone</option>
            </select>
          </div>

          <h3 class="content-title" style="margin-top: 16px;">Android</h3>
          <div class="field-row">
            <label>Min SDK</label>
            <input
              type="number"
              min="24"
              bind:value={settings.android!.min_sdk}
              placeholder={String(DEFAULT_MIN_SDK)}
            />
            <label class="short-label">Target</label>
            <input
              type="number"
              min="26"
              bind:value={settings.android!.target_sdk}
              placeholder={String(DEFAULT_TARGET_SDK)}
            />
          </div>
          <p class="field-hint">
            Min {DEFAULT_MIN_SDK} is Android 8, the oldest with the audio API the
            engine plays through. Google Play wants a recent target.
          </p>
          <div class="field-row" style="align-items: flex-start;">
            <label>ABIs</label>
            <div style="display:flex; flex-direction:column; gap:4px;">
              {#each ANDROID_ABIS as abi}
                <label class="check-option">
                  <input
                    type="checkbox"
                    checked={(settings.android!.abis?.length
                      ? settings.android!.abis
                      : ["arm64-v8a"]
                    ).includes(abi.id)}
                    onchange={(e) =>
                      toggleAbi(abi.id, (e.target as HTMLInputElement).checked)}
                  />
                  <span>{abi.label}</span>
                </label>
              {/each}
            </div>
          </div>
          <div class="field-row">
            <label>Release Keystore</label>
            <input
              type="text"
              bind:value={settings.android!.keystore}
              placeholder="release.jks (for Google Play)"
              spellcheck="false"
            />
            <label class="short-label">Alias</label>
            <input
              type="text"
              bind:value={settings.android!.key_alias}
              placeholder="upload"
              spellcheck="false"
            />
          </div>
          <p class="field-hint">
            A store build signs with this key; device builds use the debug key.
            Keep the keystore out of version control and backed up &mdash; an app
            can never be updated with a different one. Its password is asked for
            at build time and never stored.
          </p>
          <div class="field-row">
            <label>Immersive</label>
            <label class="check-option">
              <input
                type="checkbox"
                checked={settings.android!.immersive ?? true}
                onchange={(e) =>
                  (settings.android!.immersive = (e.target as HTMLInputElement).checked
                    ? undefined
                    : false)}
              />
              <span>Hide the status and navigation bars while playing</span>
            </label>
          </div>
        {:else if activeCategory === "physics"}
          <h3 class="content-title">Physics</h3>
          <div class="field-row">
            <label>Backend</label>
            <select bind:value={settings.physics.backend}>
              <option value="rapier2d">rapier2d</option>
              <option value="none">None (disabled)</option>
            </select>
          </div>
          <div class="field-row">
            <label>Gravity X (m/s²)</label>
            <input
              type="number"
              step="0.1"
              bind:value={settings.physics.gravity_x}
            />
          </div>
          <div class="field-row">
            <label>Gravity Y (m/s²)</label>
            <input
              type="number"
              step="0.1"
              bind:value={settings.physics.gravity_y}
            />
          </div>
          <div class="field-row">
            <label>Fixed Timestep</label>
            <input
              type="number"
              step="0.001"
              min="0.001"
              max="0.1"
              bind:value={settings.physics.fixed_timestep}
            />
            <span class="field-hint"
              >{(1 / settings.physics.fixed_timestep).toFixed(0)} Hz</span
            >
          </div>
          <div class="field-row">
            <label>Length Unit</label>
            <input
              type="number"
              step="0.1"
              min="0.01"
              bind:value={settings.physics.length_unit}
            />
            <span class="field-hint"
              >{settings.physics.length_unit === 1
                ? "1 unit = 1 m"
                : `${settings.physics.length_unit} units = 1 m`}</span
            >
          </div>

          {@const namedLayers = (settings.physics.collision_layer_names ?? []).filter((n) => n?.trim()).length}
          <!-- Sixteen fields nobody edits often: folded away until wanted. -->
          <details class="settings-fold">
            <summary>
              Collision Layer Names
              <span class="fold-count">{namedLayers ? `${namedLayers} of 16 named` : "none named"}</span>
            </summary>
          <div
            style="display: grid; grid-template-columns: auto 1fr; gap: 4px 8px; align-items: center;"
          >
            {#each Array(16) as _, li}
              <span style="font-size: 11px; color: var(--ed-faint); text-align: right;"
                >{li + 1}</span
              >
              <input
                type="text"
                placeholder="Layer {li + 1}"
                value={(settings.physics.collision_layer_names ?? [])[li] ?? ""}
                style="font-size: 11px; padding: 2px 4px;"
                onchange={(e) => {
                  if (!settings.physics.collision_layer_names)
                    settings.physics.collision_layer_names = [];
                  while (settings.physics.collision_layer_names.length < 16)
                    settings.physics.collision_layer_names.push("");
                  settings.physics.collision_layer_names[li] = (
                    e.target as HTMLInputElement
                  ).value;
                }}
              />
            {/each}
          </div>
          </details>

          <h4 style="margin: 12px 0 6px; color: var(--ed-text); font-size: 12px;">
            Collision Matrix
          </h4>
          <p style="font-size: 11px; color: var(--ed-faint); margin: 0 0 8px;">
            Check which layer pairs collide. Matrix is symmetric.
          </p>
          {@const matrixNames = settings.physics.collision_layer_names ?? []}
          {@const matrix =
            settings.physics.collision_matrix ?? Array(16).fill(0xffff)}
          <div style="overflow-x: auto;">
            <table class="collision-matrix">
              <thead>
                <tr>
                  <th></th>
                  {#each Array(16) as _, ci}
                    <th title={matrixNames[ci] || `Layer ${ci + 1}`}
                      >{ci + 1}</th
                    >
                  {/each}
                </tr>
              </thead>
              <tbody>
                {#each Array(16) as _, ri}
                  <tr>
                    <td
                      class="row-label"
                      title={matrixNames[ri] || `Layer ${ri + 1}`}
                      >{matrixNames[ri] || ri + 1}</td
                    >
                    {#each Array(16) as _, ci}
                      {#if ci >= ri}
                        {@const bit = 1 << ci}
                        {@const active = ((matrix[ri] ?? 0xffff) & bit) !== 0}
                        <td>
                          <button
                            class="matrix-cell"
                            class:active
                            title="{matrixNames[ri] ||
                              `Layer ${ri + 1}`} ↔ {matrixNames[ci] ||
                              `Layer ${ci + 1}`}"
                            onclick={() => {
                              if (!settings.physics.collision_matrix)
                                settings.physics.collision_matrix =
                                  Array(16).fill(0xffff);
                              // Toggle both directions (symmetric)
                              settings.physics.collision_matrix[ri] =
                                (settings.physics.collision_matrix[ri] ??
                                  0xffff) ^
                                (1 << ci);
                              if (ri !== ci) {
                                settings.physics.collision_matrix[ci] =
                                  (settings.physics.collision_matrix[ci] ??
                                    0xffff) ^
                                  (1 << ri);
                              }
                            }}
                          ></button>
                        </td>
                      {:else}
                        <td class="mirror"></td>
                      {/if}
                    {/each}
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
          <div style="display: flex; gap: 6px; margin-top: 6px;">
            <button
              class="preset-btn"
              onclick={() => {
                settings.physics.collision_matrix = Array(16).fill(0xffff);
              }}>All</button
            >
            <button
              class="preset-btn"
              onclick={() => {
                settings.physics.collision_matrix = Array(16).fill(0x0000);
              }}>None</button
            >
            <button
              class="preset-btn"
              onclick={() => {
                const m = Array(16).fill(0);
                for (let i = 0; i < 16; i++) m[i] = 1 << i;
                settings.physics.collision_matrix = m;
              }}>Diagonal Only</button
            >
          </div>
        {:else if activeCategory === "input"}
          <h3 class="content-title title-with-actions">
            Input Actions
            {#if settings.input_actions.length > 0}
              <button
                class="fold-all-btn"
                onclick={() => {
                  if (openActions.size > 0) openActions.clear();
                  else settings.input_actions.forEach((_, i) => openActions.add(i));
                }}>{openActions.size > 0 ? "Collapse all" : "Expand all"}</button
              >
            {/if}
            <button class="add-action-btn" onclick={addAction}>+ Add</button>
          </h3>
          {#each settings.input_actions as action, ai}
            {@const open = openActions.has(ai)}
            <div class="action-block" class:open>
              <div class="action-head">
                <button
                  class="fold-btn"
                  title={open ? "Collapse" : "Expand"}
                  aria-expanded={open}
                  onclick={() => toggleAction(ai)}>{open ? "▾" : "▸"}</button
                >
                <input
                  class="action-name"
                  type="text"
                  placeholder="e.g. jump"
                  bind:value={action.name}
                />
                {#if !open}
                  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                  <span
                    class="action-summary"
                    title={action.keys.join(", ")}
                    onclick={() => toggleAction(ai)}
                    >{action.keys.length ? action.keys.join(", ") : "no bindings"}</span
                  >
                {/if}
                <button class="remove-btn" title="Remove action" onclick={() => removeAction(ai)}
                  >×</button
                >
              </div>
              {#if open}
              <div class="keys-list">
                {#each action.keys as key, ki}
                  <div class="key-row">
                    <input
                      class="binding"
                      class:invalid={!isBinding(key)}
                      list="binding-names"
                      value={key}
                      title={isBinding(key) ? "A key, a pad button (Pad0_A) or a stick direction (Pad0_LeftStickX+)" : "Not a name the engine binds"}
                      onchange={(e) => setBinding(ai, ki, (e.target as HTMLInputElement).value.trim())}
                    />
                    <button
                      class="bind-btn"
                      class:on={capturing?.action === ai && capturing?.key === ki}
                      title="Press to bind: the next key, pad button or stick push becomes this binding (Escape cancels)"
                      onclick={() => startCapture(ai, ki)}
                      >{capturing?.action === ai && capturing?.key === ki ? "press…" : "⏺"}</button
                    >
                    <button
                      class="remove-btn small"
                      onclick={() => removeKeyFromAction(ai, ki)}>×</button
                    >
                  </div>
                {/each}
                <button class="add-key-btn" onclick={() => addKeyToAction(ai)}
                  >+ Key</button
                >
              </div>
              {/if}
              {#if open && hasAxis(action.keys)}
                <div class="field-row">
                  <label title="How far a stick or trigger must be pushed to count, 0 to 1 (0: the engine's default, 0.35)">Dead zone</label>
                  <input
                    type="number"
                    min="0"
                    max="1"
                    step="0.05"
                    value={action.dead_zone ?? 0}
                    onchange={(e) => {
                      const v = Math.min(1, Math.max(0, parseFloat((e.target as HTMLInputElement).value) || 0));
                      action.dead_zone = v;
                      settings.input_actions = [...settings.input_actions];
                    }}
                  />
                </div>
              {/if}
            </div>
          {/each}
          <datalist id="binding-names">
            {#each bindingChoices(4) as name}<option value={name}></option>{/each}
          </datalist>
          {#if settings.input_actions.length === 0}
            <div class="empty-hint">
              No actions defined. Scripts can use <code
                >is_action_pressed("name")</code
              >.
            </div>
          {/if}
        {:else if activeCategory === "assets"}
          {@const ap = settings.asset_pipeline!}
          <h3 class="content-title">Atlas Packing</h3>
          <div class="field-row">
            <label>Enabled</label>
            <input type="checkbox" bind:checked={ap.atlas.enabled} />
            <span class="field-hint"
              >Pack sprites into atlases during build</span
            >
          </div>
          <div class="field-row">
            <label>Max Atlas Size</label>
            <select bind:value={ap.atlas.max_atlas_size}>
              <option value={1024}>1024</option>
              <option value={2048}>2048</option>
              <option value={4096}>4096</option>
              <option value={8192}>8192</option>
            </select>
          </div>
          <div class="field-row">
            <label>Padding</label>
            <input
              type="number"
              min="0"
              max="16"
              bind:value={ap.atlas.padding}
            />
            <span class="field-hint">Pixels between regions</span>
          </div>
          <div class="field-row">
            <label>Power of Two</label>
            <input type="checkbox" bind:checked={ap.atlas.power_of_two} />
          </div>
          <div class="field-row">
            <label>Exclude Patterns</label>
            <input
              type="text"
              placeholder="e.g. ui/*, background.png"
              value={ap.atlas.exclude_patterns.join(", ")}
              onchange={(e) => {
                ap.atlas.exclude_patterns = (e.target as HTMLInputElement).value
                  .split(",")
                  .map((s) => s.trim())
                  .filter((s) => s.length > 0);
              }}
            />
          </div>

          <h3 class="content-title" style="margin-top: 16px;">
            Texture Compression
          </h3>
          <div class="field-row">
            <label>Enabled</label>
            <input type="checkbox" bind:checked={ap.compression.enabled} />
            <span class="field-hint">Compress textures during build</span>
          </div>
          <div class="field-row">
            <label>Format</label>
            <select bind:value={ap.compression.format}>
              <option value="Png">PNG (lossless)</option>
              <option value="PngOptimized">PNG Optimized (oxipng)</option>
              <option value="WebP">WebP (lossy)</option>
              <option value="Jpeg">JPEG (lossy)</option>
            </select>
          </div>

          <h3 class="content-title title-with-actions" style="margin-top: 16px;">
            Asset Packs
            <button class="add-action-btn" onclick={addPack}>+ Add</button>
          </h3>
          <p class="pkg-hint">
            A build moves each pack's files out of the game into one archive
            in its <code>packs/</code> folder; a script fetches it with
            <code>Packs.download("name")</code>. A pack can carry scripts,
            prefabs and data tables too; only input maps and the first scene
            always stay in the game. In the editor every pack is already here.
          </p>
          <div class="field-row">
            <label>Download URL</label>
            <input
              type="text"
              placeholder="packs/ beside the game"
              value={settings.asset_packs?.url ?? ""}
              onchange={(e) => {
                (settings.asset_packs ??= {}).url = (e.target as HTMLInputElement).value.trim();
              }}
            />
          </div>
          {#each settings.asset_packs?.packs ?? [] as pack, pi}
            <div class="action-block">
              <div class="field-row">
                <label>Pack</label>
                <input type="text" placeholder="e.g. forest" bind:value={pack.name} />
                <button class="remove-btn" onclick={() => removePack(pi)}>×</button>
              </div>
              <div class="field-row">
                <label title="Asset paths, one per line: a folder ending in / takes everything under it">Takes</label>
                <textarea
                  rows="3"
                  placeholder={"scenes/forest/\nsprites/forest/\nmusic/forest.ogg"}
                  value={includeText(pack.include)}
                  onchange={(e) => (pack.include = parseIncludes((e.target as HTMLTextAreaElement).value))}
                ></textarea>
              </div>
            </div>
          {/each}
          {#if packProblem(settings.asset_packs)}
            <div class="empty-hint">{packProblem(settings.asset_packs)}</div>
          {/if}
        {:else if activeCategory === "web"}
          <h3 class="content-title">Web Portal</h3>
          <p class="pkg-hint">
            The portal a web build goes to - its SDK in the page and an adapter
            the engine talks to. The engine tells it when loading is done and
            fills its loading bar (jCabinet), holds the game while it shows an
            ad or pauses it, and follows its mute. Scripts call
            <code>Platform.gameplay_start()</code>, <code>Platform.show_ad("rewarded")</code>
            and the rest; elsewhere they do nothing. The Build panel can pick
            another portal for one build. See <code>docs/web-platforms.md</code>.
          </p>
          <div class="field-row">
            <label>Portal</label>
            <select
              value={settings.web_platform ?? "none"}
              onchange={(e) => {
                const v = (e.target as HTMLSelectElement).value as NonNullable<ProjectSettings["web_platform"]>;
                settings.web_platform = v === "none" ? undefined : v;
              }}
            >
              <option value="none">None (plain web page)</option>
              <option value="jcabinet">jCabinet (jWorld)</option>
              <option value="poki">Poki</option>
              <option value="crazygames">CrazyGames</option>
              <option value="youtube">YouTube Playables</option>
            </select>
          </div>
          <div class="field-row">
            <label>Game ready</label>
            <select
              value={settings.web_game_ready ?? "first_frame"}
              onchange={(e) => {
                const v = (e.target as HTMLSelectElement).value;
                settings.web_game_ready = v === "script" ? "script" : undefined;
              }}
            >
              <option value="first_frame">On the first frame</option>
              <option value="script">When a script calls Platform.game_ready()</option>
            </select>
          </div>
          <p class="field-hint">
            When the portal hears the game can be played. The first frame suits a
            game that opens on its menu; a game with a splash or an intro first
            should say so itself - YouTube Playables fails a game that reports
            ready behind a loading screen.
          </p>
        {:else if activeCategory === "liveops"}
          <h3 class="content-title">Live Ops</h3>
          <p class="pkg-hint">
            A built game asks a live ops service (<code>jengine-server --liveops</code>)
            for its remote config, the A/B variant it is in, the live events on now
            and the current content release - and believes the answer only if it is
            signed with the key below. Scripts read values with
            <code>LiveOps.get("key")</code>; until the service answers, and offline,
            they get the defaults here. Change values on the service's dashboard,
            <code>/admin/liveops</code>.
          </p>
          <div class="field-row">
            <label>Service</label>
            <input
              type="text"
              placeholder="https://ops.example.com - empty: off"
              value={settings.liveops?.url ?? ""}
              onchange={(e) => liveopsField("url", (e.target as HTMLInputElement).value)}
            />
          </div>
          <div class="field-row">
            <label title="The service's public key: the dashboard shows it, and /liveops/v1/key">Public key</label>
            <input
              type="text"
              placeholder="jlo-public-..."
              value={settings.liveops?.public_key ?? ""}
              onchange={(e) => liveopsField("public_key", (e.target as HTMLInputElement).value)}
            />
          </div>
          <div class="field-row">
            <label>Channel</label>
            <input
              type="text"
              placeholder="live"
              value={settings.liveops?.channel ?? ""}
              onchange={(e) => liveopsField("channel", (e.target as HTMLInputElement).value)}
            />
          </div>
          <div class="field-row">
            <label title="What Play in the editor asks for">Editor channel</label>
            <input
              type="text"
              placeholder="dev"
              value={settings.liveops?.editor_channel ?? ""}
              onchange={(e) => liveopsField("editor_channel", (e.target as HTMLInputElement).value)}
            />
          </div>
          <h3 class="content-title title-with-actions" style="margin-top: 16px;">
            Defaults
            <button class="add-action-btn" onclick={addDefault}>+ Add</button>
          </h3>
          <p class="pkg-hint">
            What every value is in a game that has not heard from the service. A
            number, <code>true</code>, a list or a table is read as JSON; anything
            else as text.
          </p>
          {#each Object.entries(settings.liveops?.defaults ?? {}) as [key, value] (key)}
            <div class="field-row">
              <input
                type="text"
                value={key}
                style="max-width: 200px;"
                onchange={(e) => renameDefault(key, (e.target as HTMLInputElement).value)}
              />
              <input
                type="text"
                value={valueText(value)}
                onchange={(e) => setDefault(key, (e.target as HTMLInputElement).value)}
              />
              <button class="remove-btn" onclick={() => removeDefault(key)}>×</button>
            </div>
          {/each}
          {#if liveOpsProblem(settings.liveops)}
            <div class="empty-hint">{liveOpsProblem(settings.liveops)}</div>
          {/if}
        {:else if activeCategory === "packages"}
          <h3 class="content-title">Packages</h3>
          <p class="pkg-hint">
            Enable or disable built-in engine packages. Disabled packages won't
            register components, systems, or script APIs.
          </p>
          {#each availablePackages as pkg}
            {@const enabled = settings.enabled_packages.includes(pkg.name)}
            <div class="pkg-row" class:pkg-disabled={!enabled}>
              <label class="pkg-toggle">
                <input
                  type="checkbox"
                  checked={enabled}
                  onchange={() => togglePackage(pkg.name)}
                />
                <span class="pkg-name">{pkg.name}</span>
                <span class="pkg-version">v{pkg.version}</span>
              </label>
              <p class="pkg-desc">{pkg.description}</p>
            </div>
          {/each}
          {#if availablePackages.length === 0}
            <div class="empty-hint">Loading packages...</div>
          {/if}
        {/if}
      </div>
    </div>

    <div class="dialog-footer">
      <button class="cancel-btn" onclick={onclose}>Cancel</button>
      <button class="save-btn" onclick={handleSave}>Save</button>
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 2000;
  }

  .dialog {
    background: var(--ed-base);
    border: 1px solid var(--ed-line-strong);
    border-radius: 8px;
    width: 620px;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 14px 34px rgba(0, 0, 0, 0.45);
  }

  .dialog-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 12px 16px;
    font-size: 14px;
    font-weight: 600;
    color: var(--ed-text);
    border-bottom: 1px solid var(--ed-line);
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--ed-faint);
    font-size: 18px;
    cursor: pointer;
    padding: 0 4px;
  }

  .close-btn:hover {
    color: var(--ed-err);
  }

  .dialog-body {
    flex: 1;
    display: flex;
    overflow: hidden;
  }

  .category-nav {
    width: 160px;
    flex-shrink: 0;
    border-right: 1px solid var(--ed-line);
    padding: 8px 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .category-btn {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    border: none;
    background: none;
    color: var(--ed-faint);
    font-size: 12px;
    cursor: pointer;
    text-align: left;
    width: 100%;
    border-radius: 0;
  }

  .category-btn:hover {
    background: var(--ed-raised);
    color: var(--ed-text);
  }

  .category-btn.active {
    background: var(--ed-raised);
    color: var(--ed-text);
    border-left: 2px solid var(--ed-accent);
  }

  .cat-icon {
    font-size: 14px;
    width: 20px;
    text-align: center;
    flex-shrink: 0;
  }

  .cat-label {
    white-space: nowrap;
  }


  .content-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--ed-text);
    margin-bottom: 12px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--ed-line);
  }

  .category-content {
    flex: 1;
    overflow-y: auto;
    padding: 16px;
    /* The caption column: a hint under a row is indented to the field. */
    --label-w: 120px;
  }

  .field-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  /* The row's own caption only: a label wrapped round a checkbox inside the
     row is an option, not the label column. */
  .field-row > label {
    font-size: 12px;
    color: var(--ed-faint);
    width: var(--label-w);
    flex-shrink: 0;
  }

  /* A second caption inside a row ("Build", "Target"): as wide as its word. */
  .field-row > label.short-label {
    width: auto;
    margin-left: 8px;
  }

  /* A hint that explains the row above it: tucked under that row's field, and
     with room after it, so it reads as belonging to the field above rather
     than floating between two. */
  .field-row + .field-hint {
    margin: -4px 0 14px calc(var(--label-w) + 8px);
    line-height: 1.4;
  }

  .settings-fold {
    margin: 12px 0 6px;
  }

  .settings-fold > summary {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 6px;
    color: var(--ed-text);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    list-style: none;
    user-select: none;
  }

  .settings-fold > summary::-webkit-details-marker {
    display: none;
  }

  .settings-fold > summary::before {
    content: "▸";
    width: 10px;
    color: var(--ed-faint);
    font-size: 10px;
  }

  .settings-fold[open] > summary::before {
    content: "▾";
  }

  .fold-count {
    font-weight: 400;
    font-size: 11px;
    color: var(--ed-faint);
  }

  .check-option {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--ed-text);
  }

  .field-btn {
    flex-shrink: 0;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-text);
    padding: 4px 10px;
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
  }

  .field-btn:hover {
    background: var(--ed-line);
  }

  .field-hint {
    font-size: 11px;
    color: var(--ed-faint);
    margin-left: 4px;
  }

  .field-error {
    color: var(--ed-err);
  }

  input,
  textarea {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-text);
    padding: 4px 8px;
    font-size: 12px;
    font-family: inherit;
    flex: 1;
    min-width: 0;
  }

  textarea {
    font-family: monospace;
    resize: vertical;
  }

  input:focus,
  textarea:focus {
    outline: none;
    border-color: var(--ed-accent);
  }

  select {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-text);
    padding: 4px 8px;
    font-size: 12px;
    font-family: inherit;
    flex: 1;
    min-width: 0;
    cursor: pointer;
  }

  select:focus {
    outline: none;
    border-color: var(--ed-accent);
  }

  input[type="number"] {
    width: 70px;
  }

  /* A box, not a field: the `flex: 1` above stretched each one across its
     row and drew it in the middle of the space. */
  input[type="checkbox"] {
    flex: 0 0 auto;
    width: auto;
    margin: 0;
    padding: 0;
  }

  input[type="color"] {
    width: 36px;
    height: 28px;
    padding: 2px;
    flex: 0 0 36px;
    cursor: pointer;
    border-radius: 4px;
  }

  .dialog-footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 16px;
    border-top: 1px solid var(--ed-line);
  }

  .cancel-btn {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-text);
    padding: 6px 16px;
    font-size: 12px;
    cursor: pointer;
    font-family: inherit;
  }

  .cancel-btn:hover {
    background: var(--ed-line);
  }

  .save-btn {
    background: var(--ed-accent);
    border: none;
    border-radius: 4px;
    color: var(--ed-on-accent);
    padding: 6px 16px;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    font-family: inherit;
  }

  .save-btn:hover {
    background: var(--ed-a2);
  }

  .title-with-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .fold-all-btn {
    margin-left: auto;
    background: none;
    border: none;
    color: var(--ed-faint);
    font-size: 11.5px;
    font-family: inherit;
    cursor: pointer;
  }

  .fold-all-btn:hover {
    color: var(--ed-text);
  }

  .fold-all-btn + .add-action-btn {
    margin-left: 0;
  }

  .add-action-btn {
    margin-left: auto;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-accent);
    font-size: 11.5px;
    padding: 3px 8px;
    cursor: pointer;
    font-family: inherit;
  }

  .add-action-btn:hover {
    background: var(--ed-line);
  }

  .action-block {
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    padding: 8px;
    margin-bottom: 8px;
  }

  .action-block.open .action-head {
    margin-bottom: 6px;
  }

  .action-head {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .fold-btn {
    flex-shrink: 0;
    width: 16px;
    padding: 0;
    background: none;
    border: none;
    color: var(--ed-faint);
    font-size: 11.5px;
    cursor: pointer;
  }

  .fold-btn:hover {
    color: var(--ed-text);
  }

  .action-head .action-name {
    flex: 0 0 180px;
    font-weight: 600;
  }

  .action-summary {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    color: var(--ed-faint);
    cursor: pointer;
  }

  .action-block.open .action-head .remove-btn {
    margin-left: auto;
  }

  /* Lined up with the Dead zone field below it. */
  .keys-list {
    margin-left: calc(var(--label-w) + 8px);
  }

  .key-row {
    display: flex;
    align-items: center;
    gap: 4px;
    margin-bottom: 4px;
  }

  .key-row select {
    flex: 1;
    font-size: 11px;
    padding: 3px 6px;
  }

  .binding {
    flex: 1;
    min-width: 0;
  }
  .binding.invalid {
    border-color: var(--ed-err);
  }
  .bind-btn {
    flex-shrink: 0;
    min-width: 40px;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-faint);
    padding: 3px 6px;
    font-size: 11.5px;
    font-family: inherit;
    cursor: pointer;
  }

  .bind-btn:hover {
    color: var(--ed-text);
    background: var(--ed-line);
  }
  .bind-btn.on {
    border-color: var(--ed-warn);
    color: var(--ed-warn);
  }
  .add-key-btn {
    background: none;
    border: 1px dashed var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-faint);
    font-size: 10px;
    padding: 2px 8px;
    cursor: pointer;
    font-family: inherit;
    margin-top: 2px;
  }

  .add-key-btn:hover {
    color: var(--ed-accent);
    border-color: var(--ed-accent);
  }

  .remove-btn {
    background: none;
    border: none;
    color: var(--ed-faint);
    font-size: 16px;
    cursor: pointer;
    padding: 0 4px;
    flex-shrink: 0;
  }

  .remove-btn:hover {
    color: var(--ed-err);
  }

  .remove-btn.small {
    font-size: 14px;
  }

  .empty-hint {
    font-size: 11px;
    color: var(--ed-faint);
    padding: 4px 0;
  }

  .empty-hint code {
    background: var(--ed-raised);
    padding: 1px 4px;
    border-radius: 2px;
    font-size: 10px;
  }

  .pkg-hint {
    font-size: 11px;
    color: var(--ed-faint);
    margin-bottom: 12px;
  }

  .pkg-row {
    padding: 8px 10px;
    margin-bottom: 6px;
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
  }

  .pkg-row.pkg-disabled {
    opacity: 0.55;
  }

  .pkg-toggle {
    display: flex;
    align-items: center;
    gap: 8px;
    cursor: pointer;
    min-width: 0;
  }

  .pkg-toggle input[type="checkbox"] {
    flex: none;
    width: auto;
    min-width: auto;
  }

  .pkg-name {
    font-size: 12px;
    font-weight: 600;
    color: var(--ed-text);
  }

  .pkg-version {
    font-size: 10px;
    color: var(--ed-faint);
    margin-left: auto;
  }

  .pkg-desc {
    font-size: 11px;
    color: var(--ed-faint);
    margin: 4px 0 0 24px;
  }

  .collision-matrix {
    border-collapse: collapse;
    font-size: 10px;
  }

  .collision-matrix th,
  .collision-matrix td {
    width: 22px;
    height: 22px;
    text-align: center;
    padding: 0;
  }

  .collision-matrix th {
    color: var(--ed-faint);
    font-weight: 400;
  }

  .collision-matrix .row-label {
    text-align: right;
    padding-right: 4px;
    color: var(--ed-faint);
    font-size: 10px;
    white-space: nowrap;
    max-width: 60px;
    overflow: hidden;
    text-overflow: ellipsis;
    width: auto;
  }

  .collision-matrix .mirror {
    background: var(--ed-mantle);
  }

  .matrix-cell {
    width: 18px;
    height: 18px;
    border: 1px solid var(--ed-line-strong);
    border-radius: 2px;
    background: var(--ed-raised);
    cursor: pointer;
    padding: 0;
  }

  .matrix-cell:hover {
    border-color: var(--ed-accent);
  }

  .matrix-cell.active {
    background: var(--ed-ok);
    border-color: var(--ed-ok);
  }

  .preset-btn {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-text);
    font-size: 11.5px;
    padding: 3px 10px;
    cursor: pointer;
    font-family: inherit;
  }

  .preset-btn:hover {
    background: var(--ed-line);
  }

  /* A number is a value: mono, as the design kit sets values. */
  input[type="number"] {
    font-family: var(--ed-mono);
  }
</style>
