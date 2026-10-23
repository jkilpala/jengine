<script lang="ts">
  /**
   * File > New Rust Package: scaffold a package inside the open project
   * (`packages/<name>/`) from the engine's template, list it in
   * `project.jengine`, and say what to do next - open the code, rebuild the
   * viewport. The name is checked by the builder's own rule as it is typed,
   * so the dialog cannot offer a name the scaffold then refuses.
   */
  import { get } from "svelte/store";
  import { project } from "./stores/projectStore";
  import { logInfo, logError } from "./stores/consoleStore";
  import { createRustPackage, previewRustPackage, type RustPackagePreview } from "./api/packages";
  import { openInCodeEditor } from "./api/project";

  let {
    onclose,
    onrebuild,
  }: {
    onclose: () => void;
    /** Rebuild the editor viewport with the project's packages. */
    onrebuild: () => void;
  } = $props();

  const projectPath = get(project)?.path ?? "";

  let name = $state("");
  let preview = $state<RustPackagePreview | null>(null);
  let creating = $state(false);
  let error = $state("");
  /** `packages/<name>/src/lib.rs`, once made. */
  let created = $state<string | null>(null);

  // Ask the builder about every name as it is typed; a stale answer is dropped.
  let asked = 0;
  $effect(() => {
    const n = name.trim();
    const ticket = ++asked;
    if (!n) {
      preview = null;
      return;
    }
    previewRustPackage(projectPath, n)
      .then((p) => {
        if (ticket === asked) preview = p;
      })
      .catch(() => {});
  });

  let canCreate = $derived(!!name.trim() && !!preview && !preview.error && !creating);

  async function create() {
    if (!canCreate) return;
    creating = true;
    error = "";
    try {
      created = await createRustPackage(projectPath, name.trim());
      logInfo(`Created Rust package "${name.trim()}" at packages/${name.trim()} and listed it in project.jengine`);
    } catch (e) {
      error = `${e}`;
      logError(`New Rust package: ${e}`);
    } finally {
      creating = false;
    }
  }

  async function openCode() {
    if (!created) return;
    error = "";
    try {
      await openInCodeEditor(projectPath, created, get(project)?.settings.code_editor ?? "");
    } catch (e) {
      // Shown here as well as logged: the console is behind this dialog.
      error = `${e}`;
      logError(`Could not open ${created}: ${e}`);
    }
  }
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="overlay" onclick={onclose}>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="panel" onclick={(e) => e.stopPropagation()} role="dialog" tabindex="-1" aria-label="New Rust package">
    <div class="header">
      <h2>New Rust Package</h2>
      <button class="close-btn" onclick={onclose} aria-label="Close">×</button>
    </div>

    <div class="content">
      {#if !created}
        <p class="intro">
          A Rust crate inside this project that adds components, per-frame systems and functions your
          Lua scripts call. It starts with one of each, working, to copy from.
        </p>
        <label class="field">
          <span>Name</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input
            type="text"
            bind:value={name}
            placeholder="e.g. tide-rules"
            spellcheck="false"
            autofocus
            onkeydown={(e) => e.key === "Enter" && create()}
          />
        </label>
        <p class="note" class:bad={!!preview?.error}>
          {#if preview?.error}
            {preview.error}
          {:else if preview}
            Lands in <code>packages/{name.trim()}/</code>. Scripts call it as
            <code>{preview.namespace}.nudge(e, 10)</code>.
          {:else}
            Lowercase letters, digits and hyphens.
          {/if}
        </p>
        {#if error}<p class="note bad">{error}</p>{/if}
      {:else}
        <p class="intro">
          <strong>{name.trim()}</strong> is in <code>packages/{name.trim()}/</code> and listed in
          <code>project.jengine</code>.
        </p>
        <ol class="steps">
          <li>
            <span>Read <code>src/lib.rs</code>: a component, a system, a script command and a value scripts read back, with tests.</span>
            <button class="action-btn secondary" onclick={openCode}>Open lib.rs</button>
          </li>
          <li>
            <span>Rebuild the viewport so the editor runs it - and again after each change to the Rust (a few minutes).</span>
            <button class="action-btn" onclick={() => { onrebuild(); onclose(); }}>Rebuild now</button>
          </li>
          <li>
            <span>Add <code>{preview?.namespace ?? ""}</code>'s component to an entity in the inspector. Its scene must be version 2.</span>
          </li>
        </ol>
        {#if error}<p class="note bad flush">{error}</p>{/if}
        <p class="note flush">
          The package's files are under <code>packages</code> in the Assets panel.
          <code>cargo test</code> in the package folder runs its tests. <code>docs/writing-packages.md</code> in the engine has the rest.
        </p>
      {/if}
    </div>

    <div class="footer">
      {#if !created}
        <button class="action-btn secondary" onclick={onclose}>Cancel</button>
        <button class="action-btn" disabled={!canCreate} onclick={create}>{creating ? "Creating…" : "Create"}</button>
      {:else}
        <button class="action-btn secondary" onclick={onclose}>Done</button>
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
    width: 520px;
    max-width: calc(100vw - 32px);
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
    padding: 14px 16px;
    font-size: 12px;
    color: var(--ed-text);
  }

  .intro {
    margin: 0 0 12px;
    line-height: 1.5;
  }

  .field {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .field span {
    width: 48px;
    color: var(--ed-faint);
  }

  .field input {
    flex: 1;
    min-width: 0;
    padding: 5px 8px;
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    background: var(--ed-raised);
    color: var(--ed-text);
    font-family: var(--ed-mono, monospace);
    font-size: 12px;
  }

  .field input:focus {
    outline: none;
    border-color: var(--ed-accent);
  }

  .note {
    margin: 6px 0 0 56px;
    color: var(--ed-faint);
    line-height: 1.4;
  }

  .note.bad {
    color: var(--ed-err);
  }

  /* Under the steps, not under the name field. */
  .note.flush {
    margin-left: 0;
  }

  .steps {
    margin: 0 0 12px;
    padding-left: 18px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .steps li {
    line-height: 1.4;
  }

  .steps li span {
    display: block;
    margin-bottom: 4px;
  }

  code {
    font-family: var(--ed-mono, monospace);
    font-size: 11px;
    color: var(--ed-accent);
  }

  .footer {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    padding: 10px 16px;
    border-top: 1px solid var(--ed-line);
  }

  .action-btn {
    padding: 6px 14px;
    border: 1px solid var(--ed-accent);
    border-radius: 4px;
    background: var(--ed-accent);
    color: var(--ed-on-accent);
    cursor: pointer;
    font-size: 12px;
    font-family: inherit;
  }

  .action-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .action-btn.secondary {
    background: var(--ed-raised);
    border-color: var(--ed-line-strong);
    color: var(--ed-text);
  }

  .action-btn.secondary:hover {
    background: var(--ed-line);
  }
</style>
