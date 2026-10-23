<script lang="ts">
  import { onMount } from "svelte";
  import WindowControls from "./WindowControls.svelte";
  import { project } from "./stores/projectStore";
  import { scene, scenePath, dirty } from "./stores/sceneStore";
  import { selectedEntity, clearSelection } from "./stores/selectionStore";
  import { clearHistory } from "./stores/commandStore";
  import { logInfo, logError } from "./stores/consoleStore";
  import { loadSettings } from "./stores/editorSettingsStore";
  import {
    createProject,
    openProject,
    getRecentProjects,
    removeRecentProject,
    type RecentProject,
    type ProjectTemplate,
  } from "./api/project";
  import { createNewScene, loadScene } from "./api/scene";
  import type { ProjectInfo } from "./api/project";
  import { open } from "@tauri-apps/plugin-dialog";
  import { get } from "svelte/store";
  import { takeReopenProject } from "./viewportReload";

  let recents = $state<RecentProject[]>([]);
  let loading = $state(true);
  // "New project" first asks which template; this is that step.
  let choosingTemplate = $state(false);

  const TEMPLATES: { id: ProjectTemplate; name: string; hint: string }[] = [
    {
      id: "starter",
      name: "Starter",
      hint: "A scene with a camera, a title and a player that moves - press Play and it runs",
    },
    {
      id: "empty",
      name: "Empty",
      hint: "Project settings and empty folders - no scene, script or input action",
    },
  ];

  onMount(async () => {
    // Back from a reload that loaded another viewport module: reopen the
    // project it was for rather than leave the user here.
    const reopen = takeReopenProject();
    if (reopen) {
      await openPath(reopen);
      if (get(project)) return;
    }
    try {
      recents = await getRecentProjects();
    } catch (e) {
      console.error("Failed to load recent projects", e);
    }
    loading = false;
  });

  /**
   * Make `info` the open project, on the scene the runtime would boot into.
   *
   * `entry_scene` (and the legacy top-level `default_scene`) are stored
   * relative to `assets/`, matching how `jengine-runtime` resolves them.
   * With neither, or one that cannot be read, the editor starts on a blank
   * untitled scene.
   *
   * The scene goes into the stores **before** the project does: App's tab
   * strip adopts whatever scene is live the moment a project opens, so
   * setting the project first adopted the blank placeholder as an "Untitled
   * Scene" tab beside the entry scene that arrived a moment later.
   */
  async function enterProject(info: ProjectInfo) {
    const rel = info.settings?.entry_scene ?? info.default_scene;
    let opened: { data: Awaited<ReturnType<typeof loadScene>>; path: string | null } | null = null;
    if (rel) {
      const fullPath = `${info.path}/assets/${rel}`;
      try {
        opened = { data: await loadScene(fullPath), path: fullPath };
        logInfo(`Opened entry scene: ${rel}`);
      } catch (e) {
        logError(
          `Failed to open entry scene "${rel}": ${e} — starting with an empty scene`,
        );
      }
    }
    opened ??= { data: await createNewScene(), path: null };
    scene.set(opened.data);
    scenePath.set(opened.path);
    clearSelection();
    dirty.set(false);
    clearHistory();

    project.set(info);
    await loadSettings();
  }

  async function handleNewProject(template: ProjectTemplate) {
    const path = await open({
      directory: true,
      title: "Choose project folder",
    });
    if (!path) return;
    choosingTemplate = false;
    const dirName = (path as string).replace(/^.*[\\/]/, "");
    try {
      const info = await createProject(path as string, dirName, template);
      logInfo(`Created project: ${info.name} at ${info.path}`);
      // The template's entry scene if it has one, as opening a project does.
      await enterProject(info);
    } catch (e) {
      logError(`Failed to create project: ${e}`);
    }
  }

  async function handleOpenProject() {
    const path = await open({
      filters: [{ name: "jEngine Project", extensions: ["jengine"] }],
      title: "Open project.jengine",
    });
    if (!path) return;
    try {
      const info = await openProject(path as string);
      logInfo(`Opened project: ${info.name}`);
      await enterProject(info);
    } catch (e) {
      logError(`Failed to open project: ${e}`);
    }
  }

  /** Open a project by path, as a recent entry does; throws if it fails. */
  async function openPathOrThrow(path: string) {
    const info = await openProject(path);
    logInfo(`Opened project: ${info.name}`);
    await enterProject(info);
  }

  async function openPath(path: string) {
    try {
      await openPathOrThrow(path);
    } catch (e) {
      logError(`Failed to reopen project: ${e}`);
    }
  }

  async function handleOpenRecent(recent: RecentProject) {
    try {
      await openPathOrThrow(recent.path);
    } catch (e) {
      logError(`Failed to open project: ${e}`);
      // Offer to remove stale entry
      recents = recents.filter((r) => r.path !== recent.path);
      await removeRecentProject(recent.path).catch(() => {});
    }
  }

  async function handleRemoveRecent(e: MouseEvent, recent: RecentProject) {
    e.stopPropagation();
    recents = recents.filter((r) => r.path !== recent.path);
    await removeRecentProject(recent.path).catch(() => {});
  }
</script>

<div class="start-screen">
  <!-- The window's title bar here: the editor draws its own (no native one),
       so this strip drags the window and holds its buttons. -->
  <div class="title-strip" data-tauri-drag-region>
    <WindowControls />
  </div>

  <!-- An OVA title card, as on the website: a striped sun setting behind a
     city, over a grid floor. Decoration only. -->
  <div class="title-card">
    <div class="sky" aria-hidden="true">
      <div class="sun"></div>
      <div class="skyline">
        <span style="flex: 1; height: 50%"></span>
        <span style="flex: 0.6; height: 90%"></span>
        <span style="flex: 1.3; height: 36%"></span>
        <span style="flex: 0.5; height: 100%"></span>
        <span class="gap" style="flex: 2.6"></span>
        <span style="flex: 0.7; height: 80%"></span>
        <span style="flex: 1.2; height: 58%"></span>
        <span style="flex: 0.5; height: 70%"></span>
      </div>
      <div class="horizon"></div>
      <div class="floor"></div>
    </div>
    <div class="logo-area">
      <span class="kana" aria-hidden="true">ジェイ・エンジン</span>
      <h1 class="title">j<em>Engine</em></h1>
      <p class="subtitle">2D game engine - Rust, wgpu, Lua</p>
    </div>
  </div>

  <div class="start-content">
    <div class="heading">
      <span class="eyebrow">Start</span>
      <h2 class="heading-title">Open a project</h2>
    </div>

    {#if choosingTemplate}
      <div class="templates">
        <div class="templates-head">
          <span class="templates-title">New project from</span>
          <button class="templates-cancel" onclick={() => (choosingTemplate = false)}>Cancel</button>
        </div>
        <div class="actions">
          {#each TEMPLATES as t, i}
            <button
              class="action-btn"
              class:primary={i === 0}
              onclick={() => handleNewProject(t.id)}
            >
              <span class="action-name">{t.name}</span>
              <span class="action-hint">{t.hint}</span>
            </button>
          {/each}
        </div>
      </div>
    {:else}
      <div class="actions">
        <button class="action-btn primary" onclick={() => (choosingTemplate = true)}>
          <span class="action-name">New project</span>
          <span class="action-hint">Starter or empty</span>
        </button>
        <button class="action-btn" onclick={handleOpenProject}>
          <span class="action-name">Open project</span>
          <span class="action-hint">A folder with project.jengine</span>
        </button>
      </div>
    {/if}

    <div class="recents-section">
      <div class="recents-head">
        <h2 class="recents-title">Recent projects</h2>
        {#if !loading && recents.length > 0}
          <span class="recents-count">{recents.length}</span>
        {/if}
      </div>
      {#if loading}
        <p class="empty">Loading...</p>
      {:else if recents.length === 0}
        <p class="empty">No recent projects</p>
      {:else}
        <ul class="recents-list">
          {#each recents as recent, i}
            <li>
              <div
                class="recent-item"
                role="button"
                tabindex="0"
                onclick={() => handleOpenRecent(recent)}
                onkeydown={(e) => {
                  if (e.key === "Enter") handleOpenRecent(recent);
                }}
              >
                <span class="recent-tile" data-tone={i % 3} aria-hidden="true"
                  >{(recent.name || "?").charAt(0).toUpperCase()}</span
                >
                <div class="recent-info">
                  <span class="recent-name">{recent.name}</span>
                  <span class="recent-path">{recent.path}</span>
                </div>
                <button
                  class="remove-btn"
                  title="Remove from recent"
                  aria-label="Remove {recent.name} from recent projects"
                  onclick={(e) => handleRemoveRecent(e, recent)}>×</button
                >
              </div>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>
</div>

<style>
  .title-strip {
    position: fixed;
    top: 0;
    left: 0;
    right: 0;
    height: 36px;
    display: flex;
    justify-content: flex-end;
    padding-right: 10px; /* the controls' -10px margin puts them at the edge */
    z-index: 10;
    user-select: none;
    -webkit-user-select: none;
  }

  .start-screen {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(420px, 540px);
    height: 100vh;
    background: var(--ed-base);
  }

  /* ---- Title card ---- */
  .title-card {
    position: relative;
    overflow: hidden;
    background: linear-gradient(
      180deg,
      var(--ed-base) 0%,
      color-mix(in srgb, var(--ed-accent-low) 70%, var(--ed-base)) 58%,
      var(--ed-base) 100%
    );
  }

  .sky {
    position: absolute;
    inset: 0;
  }

  .sun {
    position: absolute;
    left: 50%;
    top: 18%;
    width: min(320px, 34vh);
    aspect-ratio: 1;
    transform: translateX(-50%);
    border-radius: 50%;
    background: linear-gradient(180deg, var(--ed-sun1), var(--ed-sun2));
    box-shadow: 0 0 160px color-mix(in srgb, var(--ed-sun2) 35%, transparent);
    -webkit-mask: linear-gradient(180deg, #000 52%, transparent 52% 59%, #000 59% 70%, transparent 70% 77%, #000 77% 85%, transparent 85% 90%, #000 90%);
    mask: linear-gradient(180deg, #000 52%, transparent 52% 59%, #000 59% 70%, transparent 70% 77%, #000 77% 85%, transparent 85% 90%, #000 90%);
  }

  .skyline {
    position: absolute;
    left: 0;
    right: 0;
    top: 46%;
    height: 17%;
    display: flex;
    align-items: flex-end;
    gap: 8px;
    padding: 0 5%;
  }

  .skyline span {
    background: var(--ed-base);
  }

  .skyline .gap {
    background: none;
  }

  .horizon {
    position: absolute;
    left: 0;
    right: 0;
    top: 63%;
    height: 1px;
    background: var(--ed-accent);
    box-shadow: 0 0 22px var(--ed-accent);
  }

  .floor {
    position: absolute;
    left: -40%;
    right: -40%;
    top: 63%;
    height: 60%;
    background-image:
      linear-gradient(var(--ed-line) 1px, transparent 1px),
      linear-gradient(90deg, var(--ed-line) 1px, transparent 1px);
    background-size: 64px 64px;
    transform: perspective(420px) rotateX(60deg);
    transform-origin: 50% 0;
  }

  .logo-area {
    position: absolute;
    z-index: 1;
    left: 56px;
    bottom: 56px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .kana {
    font-family: var(--ed-display);
    font-size: 14px;
    letter-spacing: 0.4em;
    color: var(--ed-a2);
  }

  .title {
    margin: 0;
    font-family: var(--ed-display);
    font-weight: 400;
    font-size: clamp(56px, 9vw, 92px);
    line-height: 0.92;
    padding-bottom: 0.14em;
    color: var(--ed-ink);
  }

  .title em {
    font-style: normal;
    color: var(--ed-accent);
    text-shadow: 0 0 30px color-mix(in srgb, var(--ed-accent) 55%, transparent);
  }

  .subtitle {
    margin: 0;
    font-family: var(--ed-mono);
    font-size: 11.5px;
    letter-spacing: 0.16em;
    text-transform: uppercase;
    color: var(--ed-a3);
  }

  /* ---- Projects ---- */
  .start-content {
    display: flex;
    flex-direction: column;
    gap: 28px;
    padding: 64px 48px;
    min-height: 0;
    background: var(--ed-mantle);
    border-left: 1px solid var(--ed-line);
  }

  .heading {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .eyebrow {
    font-family: var(--ed-mono);
    font-size: 11px;
    letter-spacing: 0.16em;
    text-transform: uppercase;
    color: var(--ed-a3);
  }

  .heading-title {
    margin: 0;
    font-family: var(--ed-display);
    font-weight: 400;
    font-size: 30px;
    color: var(--ed-ink);
  }

  .actions {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
  }

  .templates {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .templates-head {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
  }

  .templates-title {
    font-family: var(--ed-mono);
    font-size: 11px;
    letter-spacing: 0.16em;
    text-transform: uppercase;
    color: var(--ed-muted);
  }

  .templates-cancel {
    background: none;
    border: none;
    padding: 0;
    font-family: inherit;
    font-size: 12px;
    color: var(--ed-muted);
    cursor: pointer;
  }

  .templates-cancel:hover {
    color: var(--ed-ink);
  }

  .action-btn {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 4px;
    padding: 14px 16px;
    font-family: inherit;
    text-align: left;
    color: var(--ed-ink);
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 8px;
    cursor: pointer;
    transition: background 0.15s, filter 0.15s;
  }

  .action-btn:hover {
    background: var(--ed-hover);
  }

  .action-btn.primary {
    background: var(--ed-accent);
    border-color: var(--ed-accent);
    color: var(--ed-on-accent);
  }

  .action-btn.primary:hover {
    filter: brightness(1.08);
  }

  .action-name {
    font-weight: 600;
    font-size: 14px;
  }

  .action-hint {
    font-size: 12px;
    color: var(--ed-muted);
  }

  .action-btn.primary .action-hint {
    color: inherit;
    opacity: 0.8;
  }

  .recents-section {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 0;
  }

  .recents-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }

  .recents-title,
  .recents-count {
    margin: 0;
    font-family: var(--ed-mono);
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.12em;
    text-transform: uppercase;
    color: var(--ed-faint);
  }

  .empty {
    color: var(--ed-faint);
    font-size: 13px;
    text-align: center;
    padding: 24px 0;
  }

  .recents-list {
    list-style: none;
    margin: 0;
    padding: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .recent-item {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 10px 12px;
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    border-radius: 7px;
    cursor: pointer;
    text-align: left;
    color: var(--ed-text);
    font-family: inherit;
    font-size: 13px;
    transition: background 0.12s, border-color 0.12s;
  }

  .recent-item:hover {
    background: var(--ed-hover);
    border-color: var(--ed-line-strong);
  }

  .recent-tile {
    width: 34px;
    height: 34px;
    flex-shrink: 0;
    border-radius: 6px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-family: var(--ed-display);
    font-size: 14px;
    color: var(--ed-on-accent);
    background: var(--ed-accent);
  }

  .recent-tile[data-tone="1"] {
    background: var(--ed-a2);
  }

  .recent-tile[data-tone="2"] {
    background: var(--ed-a3);
  }

  .recent-info {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .recent-name {
    font-weight: 600;
    color: var(--ed-ink);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .recent-path {
    font-family: var(--ed-mono);
    font-size: 11px;
    color: var(--ed-faint);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .remove-btn {
    flex-shrink: 0;
    width: 24px;
    height: 24px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: none;
    border: none;
    border-radius: 4px;
    color: var(--ed-faint);
    font-size: 16px;
    cursor: pointer;
    opacity: 0;
    transition:
      opacity 0.12s,
      color 0.12s;
  }

  .recent-item:hover .remove-btn,
  .remove-btn:focus-visible {
    opacity: 1;
  }

  .remove-btn:hover {
    color: var(--ed-err);
    background: color-mix(in srgb, var(--ed-err) 10%, transparent);
  }

  @media (max-width: 900px) {
    .start-screen {
      grid-template-columns: minmax(0, 1fr);
    }

    .title-card {
      display: none;
    }
  }
</style>
