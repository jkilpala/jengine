<script lang="ts">
  import { get } from "svelte/store";
  import { open } from "@tauri-apps/plugin-dialog";
  import { project } from "./stores/projectStore";
  import { logInfo, logError } from "./stores/consoleStore";
  import {
    listPackages,
    addPackage,
    removePackage,
    resolvePackages,
    getAvailablePackages,
    detectPackageInfo,
    type PackageInfo,
    type AvailablePackage,
  } from "./api/packages";
  import { saveProject } from "./api/project";
  import { reloadPackageSchema } from "./stores/packageSchemaStore";

  let {
    onclose,
    onnewrustpackage,
  }: {
    onclose: () => void;
    /** Open New Rust Package (this dialog closes first). */
    onnewrustpackage?: () => void;
  } = $props();

  let packages = $state<PackageInfo[]>([]);
  let builtinPackages = $state<AvailablePackage[]>([]);
  let enabledPackages = $state<string[]>(
    get(project)?.settings.enabled_packages ?? [],
  );
  let loading = $state(true);
  let resolving = $state(false);
  let showAddForm = $state(false);

  // Add form fields
  let newName = $state("");
  let newSourceType = $state<"path" | "git">("path");
  let newSourceLocation = $state("");
  let newTag = $state("");
  let adding = $state(false);
  let detecting = $state(false);

  let projectPath = $derived(get(project)?.path ?? "");

  async function loadPackages() {
    if (!projectPath) return;
    loading = true;
    try {
      packages = await listPackages(projectPath);
      builtinPackages = await getAvailablePackages();
    } catch (e) {
      logError(`Failed to list packages: ${e}`);
    } finally {
      loading = false;
    }
  }

  async function toggleBuiltin(name: string) {
    const proj = get(project);
    if (!proj) return;
    const idx = enabledPackages.indexOf(name);
    if (idx >= 0) {
      enabledPackages = enabledPackages.filter((n) => n !== name);
    } else {
      enabledPackages = [...enabledPackages, name];
    }
    // Persist to project settings
    const updatedSettings = {
      ...proj.settings,
      enabled_packages: enabledPackages,
    };
    try {
      await saveProject(
        proj.path,
        proj.name,
        proj.default_scene,
        updatedSettings,
        proj.component_definitions ?? [],
      );
      project.update((p) => (p ? { ...p, settings: updatedSettings } : p));
      // Reload schema so Inspector/Toolbar/panels update immediately
      await reloadPackageSchema(enabledPackages, proj.path);
      logInfo(
        `Package "${name}" ${enabledPackages.includes(name) ? "enabled" : "disabled"}`,
      );
    } catch (e) {
      logError(`Failed to save package toggle: ${e}`);
    }
  }

  async function handleResolve() {
    if (!projectPath) return;
    resolving = true;
    try {
      packages = await resolvePackages(projectPath);
      logInfo(`Resolved ${packages.length} package(s)`);
    } catch (e) {
      logError(`Failed to resolve packages: ${e}`);
    } finally {
      resolving = false;
    }
  }

  async function handleAdd() {
    if (!projectPath || !newName.trim() || !newSourceLocation.trim()) return;
    adding = true;
    try {
      await addPackage(
        projectPath,
        newName.trim(),
        newSourceType,
        newSourceLocation.trim(),
        newTag.trim() || null,
        false,
      );
      logInfo(`Added package: ${newName.trim()}`);
      showAddForm = false;
      newName = "";
      newSourceLocation = "";
      newTag = "";
      await loadPackages();
    } catch (e) {
      logError(`Failed to add package: ${e}`);
    } finally {
      adding = false;
    }
  }

  async function handleRemove(name: string) {
    if (!projectPath) return;
    if (!confirm(`Remove package "${name}" from the project?`)) return;
    try {
      const removed = await removePackage(projectPath, name);
      if (removed) {
        logInfo(`Removed package: ${name}`);
        await loadPackages();
      } else {
        logError(`Package "${name}" not found`);
      }
    } catch (e) {
      logError(`Failed to remove package: ${e}`);
    }
  }

  async function browsePath() {
    const dir = await open({
      directory: true,
      title: "Select package folder",
    });
    if (dir) {
      newSourceLocation = dir as string;
      await tryDetectName(dir as string);
    }
  }

  async function tryDetectName(path: string) {
    if (!path.trim()) return;
    detecting = true;
    try {
      const info = await detectPackageInfo(path.trim());
      newName = info.name;
    } catch {
      // No package.toml or Cargo.toml — user must type name
    } finally {
      detecting = false;
    }
  }

  // Load on mount
  $effect(() => {
    loadPackages();
  });
</script>

<div class="overlay" role="dialog" aria-label="Package Manager">
  <div class="panel">
    <div class="header">
      <h2>📦 Package Manager</h2>
      <button class="close-btn" onclick={onclose} aria-label="Close">✕</button>
    </div>

    <div class="content">
      <!-- Built-in engine packages -->
      <div class="section-header">Engine Packages</div>
      {#if builtinPackages.length === 0 && loading}
        <div class="empty-state">Loading…</div>
      {:else}
        <div class="package-list">
          {#each builtinPackages as pkg (pkg.name)}
            {@const enabled = enabledPackages.includes(pkg.name)}
            <div class="package-card" class:disabled-pkg={!enabled}>
              <div class="package-header">
                <label class="toggle-label">
                  <input
                    type="checkbox"
                    checked={enabled}
                    onchange={() => toggleBuiltin(pkg.name)}
                  />
                </label>
                <span class="package-name">{pkg.name}</span>
                <span class="package-version">v{pkg.version}</span>
                <span class="badge source-type">built-in</span>
              </div>
              {#if pkg.description}
                <div class="package-desc">{pkg.description}</div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}

      <!-- External / third-party packages -->
      <div class="section-header" style="margin-top: 16px;">
        External Packages
      </div>
      <div class="toolbar-row">
        <button class="action-btn" onclick={() => (showAddForm = !showAddForm)}>
          {showAddForm ? "Cancel" : "+ Add Package"}
        </button>
        {#if onnewrustpackage}
          <button
            class="action-btn"
            title="Scaffold a Rust package inside this project and list it here"
            onclick={() => {
              onclose();
              onnewrustpackage();
            }}>+ New Rust Package</button
          >
        {/if}
        <button
          class="action-btn secondary"
          onclick={handleResolve}
          disabled={resolving}
        >
          {resolving ? "Resolving…" : "🔄 Resolve All"}
        </button>
      </div>

      {#if showAddForm}
        <div class="add-form">
          <div class="form-row">
            <label for="pkg-source-type">Source</label>
            <select id="pkg-source-type" bind:value={newSourceType}>
              <option value="path">Local Path</option>
              <option value="git">Git Repository</option>
            </select>
          </div>
          <div class="form-row">
            <label for="pkg-location">
              {newSourceType === "path" ? "Path" : "URL"}
            </label>
            <input
              id="pkg-location"
              type="text"
              bind:value={newSourceLocation}
              onblur={() =>
                newSourceType === "path" && tryDetectName(newSourceLocation)}
              placeholder={newSourceType === "path"
                ? "../packages/my-package"
                : "https://github.com/user/package.git"}
            />
            {#if newSourceType === "path"}
              <button
                class="browse-btn"
                onclick={browsePath}
                type="button"
                title="Browse for package folder">Browse…</button
              >
            {/if}
          </div>
          <div class="form-row">
            <label for="pkg-name">Name</label>
            <input
              id="pkg-name"
              type="text"
              bind:value={newName}
              placeholder={detecting ? "Detecting…" : "package-name"}
            />
          </div>
          {#if newSourceType === "git"}
            <div class="form-row">
              <label for="pkg-tag">Tag</label>
              <input
                id="pkg-tag"
                type="text"
                bind:value={newTag}
                placeholder="v1.0.0 (optional)"
              />
            </div>
          {/if}
          <button
            class="action-btn"
            onclick={handleAdd}
            disabled={adding || !newName.trim() || !newSourceLocation.trim()}
          >
            {adding ? "Adding…" : "Add Package"}
          </button>
        </div>
      {/if}

      {#if loading}
        <div class="empty-state">Loading packages…</div>
      {:else if packages.length === 0}
        <div class="empty-state">
          No packages installed. Click "+ Add Package" to get started.
        </div>
      {:else}
        <div class="package-list">
          {#each packages as pkg (pkg.name)}
            <div class="package-card">
              <div class="package-header">
                <span class="package-name">{pkg.name}</span>
                {#if pkg.version}
                  <span class="package-version">v{pkg.version}</span>
                {/if}
                {#if pkg.optional}
                  <span class="badge optional">optional</span>
                {/if}
                <span class="badge source-type">{pkg.source_type}</span>
              </div>
              {#if pkg.description}
                <div class="package-desc">{pkg.description}</div>
              {/if}
              <div class="package-meta">
                <span class="meta-item" title={pkg.source_location}>
                  📍 {pkg.source_location}
                </span>
                {#if pkg.tag}
                  <span class="meta-item">🏷️ {pkg.tag}</span>
                {/if}
                {#if pkg.author}
                  <span class="meta-item">👤 {pkg.author}</span>
                {/if}
              </div>
              <div class="package-actions">
                <button
                  class="remove-btn"
                  onclick={() => handleRemove(pkg.name)}
                  title="Remove package"
                >
                  Remove
                </button>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</div>

<style>
  .overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.6);
    display: flex;
    align-items: center;
    justify-content: center;
    z-index: 100;
  }

  .panel {
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    border-radius: 8px;
    width: 600px;
    max-height: 80vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
  }

  .header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 12px 16px;
    border-bottom: 1px solid var(--ed-line);
  }

  .header h2 {
    margin: 0;
    font-size: 14px;
    color: var(--ed-text);
  }

  .close-btn {
    background: none;
    border: none;
    color: var(--ed-text);
    cursor: pointer;
    font-size: 16px;
    padding: 4px;
    border-radius: 4px;
  }

  .close-btn:hover {
    background: var(--ed-raised);
  }

  .content {
    padding: 12px 16px;
    overflow-y: auto;
    flex: 1;
  }

  .toolbar-row {
    display: flex;
    gap: 8px;
    margin-bottom: 12px;
  }

  .action-btn {
    padding: 6px 12px;
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    background: var(--ed-accent-low);
    color: var(--ed-accent);
    cursor: pointer;
    font-size: 12px;
  }

  .action-btn:hover:not(:disabled) {
    background: color-mix(in srgb, var(--ed-accent) 20%, transparent);
  }

  .action-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .action-btn.secondary {
    background: transparent;
    color: var(--ed-text);
  }

  .add-form {
    background: rgba(0, 0, 0, 0.2);
    border: 1px solid var(--ed-line);
    border-radius: 6px;
    padding: 12px;
    margin-bottom: 12px;
  }

  .form-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  .form-row label {
    width: 60px;
    font-size: 12px;
    color: var(--ed-muted);
    flex-shrink: 0;
  }

  .form-row input[type="text"],
  .form-row select {
    flex: 1;
    padding: 4px 8px;
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    background: var(--ed-mantle);
    color: var(--ed-text);
    font-size: 12px;
  }


  .empty-state {
    text-align: center;
    padding: 24px;
    color: var(--ed-faint);
    font-size: 13px;
  }

  .package-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .package-card {
    border: 1px solid var(--ed-line);
    border-radius: 6px;
    padding: 10px 12px;
    background: rgba(0, 0, 0, 0.15);
  }

  .package-header {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 4px;
  }

  .package-name {
    font-size: 13px;
    font-weight: 600;
    color: var(--ed-text);
  }

  .package-version {
    font-size: 11px;
    color: var(--ed-muted);
  }

  .badge {
    font-size: 10px;
    padding: 1px 6px;
    border-radius: 3px;
    text-transform: uppercase;
  }

  .badge.optional {
    background: color-mix(in srgb, var(--ed-warn) 15%, transparent);
    color: var(--ed-warn);
  }

  .badge.source-type {
    background: color-mix(in srgb, var(--ed-accent) 15%, transparent);
    color: var(--ed-accent);
  }

  .package-desc {
    font-size: 12px;
    color: var(--ed-muted);
    margin-bottom: 4px;
  }

  .package-meta {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    margin-bottom: 6px;
  }

  .meta-item {
    font-size: 11px;
    color: var(--ed-faint);
    max-width: 250px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .package-actions {
    display: flex;
    justify-content: flex-end;
  }

  .remove-btn {
    padding: 3px 10px;
    border: 1px solid var(--ed-err);
    border-radius: 4px;
    background: transparent;
    color: var(--ed-err);
    cursor: pointer;
    font-size: 11px;
  }

  .remove-btn:hover {
    background: color-mix(in srgb, var(--ed-err) 15%, transparent);
  }

  .section-header {
    font-size: 11px;
    font-weight: 600;
    color: var(--ed-faint);
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 8px;
  }

  .toggle-label {
    display: flex;
    align-items: center;
    cursor: pointer;
    flex-shrink: 0;
  }

  .toggle-label input[type="checkbox"] {
    margin: 0;
  }

  .disabled-pkg {
    opacity: 0.5;
  }

  .browse-btn {
    padding: 4px 10px;
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    background: var(--ed-accent-low);
    color: var(--ed-accent);
    cursor: pointer;
    font-size: 12px;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .browse-btn:hover {
    background: color-mix(in srgb, var(--ed-accent) 20%, transparent);
  }
</style>
