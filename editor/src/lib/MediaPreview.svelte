<script lang="ts">
  // A preview of a video (`.mp4`, `.webm`) or a font file (`.ttf`, `.otf`)
  // from the asset browser. A video plays as the `<video>` the browser hosts
  // decode it with (`video::web`) - its size, length and whether it loops
  // cleanly are what to check; a font is loaded with the FontFace API and
  // shown at the sizes a `text` draws, with a line of each script a game
  // might need, so a missing glyph shows before the font is named in a
  // scene (`renderer::dynamic_font` bakes glyphs from the same file).
  import { readFile } from "@tauri-apps/plugin-fs";

  let { projectPath, relativePath, onclose }: { projectPath: string; relativePath: string; onclose: () => void } =
    $props();

  const ext = $derived(relativePath.split(".").pop()?.toLowerCase() ?? "");
  const isVideo = $derived(ext === "mp4" || ext === "webm" || ext === "mov");
  const fileName = $derived(relativePath.split("/").pop() ?? relativePath);

  let url = $state<string | null>(null);
  let error = $state("");
  let bytes = $state(0);
  let family = $state("");
  let video = $state<HTMLVideoElement | null>(null);
  let info = $state("");
  let sample = $state("The quick brown fox jumps over the lazy dog 0123456789");

  // Lines a game might show: does this file draw them?
  const SCRIPTS: [string, string][] = [
    ["Latin", "Ääkkösiä, ñandú, Straße, łódź, Ærøskøbing"],
    ["Greek", "Καλημέρα κόσμε"],
    ["Cyrillic", "Съешь же ещё этих мягких булок"],
    ["Japanese", "言霊 ことだま コトダマ"],
    ["Arabic", "مرحبا بالعالم"],
    ["Symbols", "← → ↑ ↓ ★ ♥ ✓ × ÷ € £ ¥ © ™"],
  ];

  let blobUrl: string | null = null;
  $effect(() => {
    void load(projectPath, relativePath);
    return () => {
      if (blobUrl) URL.revokeObjectURL(blobUrl);
      blobUrl = null;
    };
  });

  async function load(proj: string, rel: string) {
    error = "";
    try {
      const data = await readFile(`${proj}/${rel}`);
      bytes = data.byteLength;
      if (isVideo) {
        const mime = ext === "webm" ? "video/webm" : ext === "mov" ? "video/quicktime" : "video/mp4";
        if (blobUrl) URL.revokeObjectURL(blobUrl);
        blobUrl = URL.createObjectURL(new Blob([data], { type: mime }));
        url = blobUrl;
      } else {
        // A family name of its own, so two previews never share a face.
        const name = `jengine-preview-${Math.random().toString(36).slice(2)}`;
        const face = new FontFace(name, data);
        await face.load();
        document.fonts.add(face);
        family = name;
      }
    } catch (e) {
      error = `Could not load ${rel}: ${e}`;
    }
  }

  function videoInfo() {
    const v = video;
    if (!v) return;
    info = `${v.videoWidth} x ${v.videoHeight}, ${v.duration.toFixed(2)} s`;
  }

  const kb = (n: number) => (n > 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.round(n / 1024)} KB`);
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="overlay" role="dialog" aria-label="Media preview" tabindex="-1" onkeydown={(e) => e.key === "Escape" && onclose()}>
  <div class="window">
    <div class="head">
      <span class="title">{isVideo ? "🎞" : "🔤"} {fileName}</span>
      <span class="dim">{kb(bytes)}{info ? ` · ${info}` : ""}</span>
      <span class="spacer"></span>
      <button onclick={onclose} title="Close">×</button>
    </div>
    {#if error}
      <div class="error">{error}</div>
    {:else if isVideo}
      <div class="video">
        {#if url}
          <!-- svelte-ignore a11y_media_has_caption -->
          <video bind:this={video} src={url} controls loop onloadedmetadata={videoInfo}></video>
        {/if}
        <div class="dim">Played as the editor and web builds play it; natively through Media Foundation. A video in a scene is silent - its sound is a separate music track.</div>
      </div>
    {:else if family}
      <div class="font">
        <input class="sample-input" bind:value={sample} placeholder="Type to try it" />
        {#each [12, 16, 24, 36, 56] as size}
          <div class="sample" style="font-family:'{family}';font-size:{size}px"><span class="size">{size}</span>{sample}</div>
        {/each}
        <div class="scripts">
          {#each SCRIPTS as [name, line]}
            <div class="row"><span class="dim">{name}</span><span style="font-family:'{family}', monospace">{line}</span></div>
          {/each}
        </div>
        <div class="dim">A glyph the file lacks falls back to another font here (it looks different); in a game it draws nothing. Name this file as a text's <code>font_id</code> to use it.</div>
      </div>
    {:else}
      <div class="dim pad">Loading…</div>
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
  }
  .window {
    width: min(900px, 94vw);
    max-height: 90vh;
    overflow: auto;
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
  .spacer {
    flex: 1;
  }
  .dim {
    color: var(--ed-faint);
  }
  .pad {
    padding: 16px;
  }
  .error {
    color: var(--ed-err);
    padding: 12px;
  }
  .video {
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  video {
    max-width: 100%;
    max-height: 70vh;
    background: #000;
  }
  .font {
    padding: 10px;
    display: flex;
    flex-direction: column;
    gap: 6px;
  }
  .sample {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .size {
    display: inline-block;
    width: 28px;
    font-family: sans-serif;
    font-size: 10px;
    color: var(--ed-faint);
  }
  .sample-input {
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    border-radius: 4px;
    padding: 3px 6px;
  }
  .scripts {
    display: flex;
    flex-direction: column;
    gap: 3px;
    margin-top: 6px;
    font-size: 16px;
  }
  .row {
    display: flex;
    gap: 10px;
  }
  .row .dim {
    width: 70px;
    font-size: 11px;
  }
  button {
    background: none;
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    border-radius: 4px;
    cursor: pointer;
  }
</style>
