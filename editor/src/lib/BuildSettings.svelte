<script lang="ts">
  import { get } from "svelte/store";
  import { project } from "./stores/projectStore";
  import { editorSettings, updateSetting } from "./stores/editorSettingsStore";
  import { WASM_OPT_LEVELS, type WasmOptLevel } from "./api/editorPrefs";
  import { logInfo, logError } from "./stores/consoleStore";
  import { beginBuild, endBuild, buildRunning } from "./stores/buildStore";
  import { focusPanel } from "./stores/layoutStore";
  import { checkToolsAfterBuildFailure } from "./api/buildTools";
  import {
    buildWindows,
    buildWeb,
    buildAndroid,
    buildDesktop,
    buildIos,
    openBuildReport,
    openBuildFolder,
  } from "./api/build";
  import { estimateSavings, type SavingsEstimate } from "./api/spritesheet";
  import { open } from "@tauri-apps/plugin-dialog";

  let { onclose }: { onclose: () => void } = $props();

  // Initialize from persisted editor settings
  const es = get(editorSettings);
  let target = $state<Category>(es.build_target as Category);
  let profile = $state<"debug" | "release">(
    es.build_profile as "debug" | "release",
  );
  let building = $state(false);

  type Category = "windows" | "linux" | "macos" | "web" | "android" | "ios";
  let activeCategory = $state<Category>(es.build_target as Category);

  const categories: { id: Category; label: string; icon: string }[] = [
    { id: "windows", label: "Windows", icon: "🪟" },
    { id: "linux", label: "Linux", icon: "🐧" },
    { id: "macos", label: "macOS", icon: "🍎" },
    { id: "web", label: "Web", icon: "🌐" },
    { id: "android", label: "Android", icon: "📱" },
    { id: "ios", label: "iOS", icon: "📲" },
  ];

  const TARGET_LABELS: Record<Category, string> = {
    windows: "Windows",
    linux: "Linux",
    macos: "macOS",
    web: "Web",
    android: "Android",
    ios: "iOS",
  };

  // A macOS build is made on a Mac (Apple's linker and SDK); from anywhere
  // else the tab explains rather than offering a build that cannot run.
  const onMac = /Mac/i.test(navigator.userAgent);
  const onWindows = /Windows/i.test(navigator.userAgent);

  // iOS: the simulator unless a device is chosen; a device build is signed
  // with a development identity and a provisioning profile, kept for this
  // dialog only.
  let iosDevice = $state(false);
  let iosInstall = $state(true);
  let iosLaunch = $state(true);
  let iosIdentity = $state("");
  let iosProfile = $state("");

  async function pickIosProfile() {
    const file = await open({
      title: "Choose a provisioning profile",
      filters: [{ name: "Provisioning profile", extensions: ["mobileprovision"] }],
    });
    if (file) iosProfile = file as string;
  }

  // Android: hand the APK to the connected phone / emulator. Not persisted -
  // whether a device is plugged in changes from one build to the next.
  let androidInstall = $state(true);
  let androidLaunch = $state(true);
  // A store build: the App Bundle, signed with the project's own key. The
  // password lives in this dialog for one build and is never written down.
  let androidBundle = $state(false);
  let androidReleaseKey = $state(false);
  let androidKeystorePassword = $state("");

  // Keep target in sync with active category and persist
  $effect(() => {
    target = activeCategory;
    updateSetting("build_target", activeCategory);
  });

  let outputDir = $state<string>(es.build_output_dir);
  let generateReport = $state(es.build_generate_report);
  let atlasEnabled = $state(true);
  let atlasMaxSize = $state(2048);
  let atlasPadding = $state(1);
  let compressionEnabled = $state(false);
  let compressionFormat = $state<
    "png_optimized" | "webp_lossless" | "webp_lossy" | "basis_universal"
  >("png_optimized");
  // Default on - see the note on `EditorSettings` in the Tauri backend;
  // overwritten by whatever the project's editor-settings.json says.
  let wasmLto = $state(es.wasm_lto ?? true);
  let wasmOptSize = $state(es.wasm_opt_size ?? true);
  let wasmSingleCodegen = $state(es.wasm_single_codegen ?? true);
  // The wasm-opt pass after a release web build - this project's own level,
  // apart from the editor viewport's (Project Settings -> Graphics).
  let wasmOptLevel = $state<WasmOptLevel>(es.wasm_opt_level ?? "optimized");
  let enableProfiling = $state(es.build_enable_profiling);
  // Ships the F3 stats HUD / F12 inspector with the game. On unless a
  // project turned it off - see `EditorSettings` in the Tauri backend.
  let debugOverlay = $state(es.build_debug_overlay ?? true);
  // The web portal for this build. "project" is the project's own
  // (Project Settings -> Web); the same game is often built for several, so
  // the choice is per build and not kept.
  const WEB_PLATFORMS: { id: string; label: string }[] = [
    { id: "project", label: "Project setting" },
    { id: "none", label: "None (plain web page)" },
    { id: "jcabinet", label: "jCabinet (jWorld)" },
    { id: "poki", label: "Poki" },
    { id: "crazygames", label: "CrazyGames" },
    { id: "youtube", label: "YouTube Playables" },
  ];
  let webPlatform = $state("project");
  let estimating = $state(false);
  let savingsResult = $state<SavingsEstimate | null>(null);

  // Persist profile changes
  $effect(() => {
    updateSetting("build_profile", profile);
  });

  // Persist profiling toggle
  $effect(() => {
    updateSetting("build_enable_profiling", enableProfiling);
  });

  // Persist debug-overlay toggle
  $effect(() => {
    updateSetting("build_debug_overlay", debugOverlay);
  });

  async function pickOutputDir() {
    const dir = await open({
      directory: true,
      title: "Choose build output folder",
    });
    if (dir) {
      outputDir = dir as string;
      updateSetting("build_output_dir", outputDir);
    }
  }

  function resolvedOutputDir(): string {
    if (!outputDir) return "";
    return `${outputDir}/${target}/${profile}`;
  }

  async function handleBuild() {
    const proj = get(project);
    if (!proj) {
      logError("Open a project first before building");
      return;
    }

    if (get(buildRunning)) {
      logError("A build is already running — watch the Build panel");
      return;
    }

    if (!outputDir) {
      await pickOutputDir();
      if (!outputDir) return;
    }
    const finalOutputDir = `${outputDir}/${target}/${profile}`;

    // Persist current state before building
    updateSetting("build_generate_report", generateReport);

    building = true;
    const release = profile === "release";
    const label = `${TARGET_LABELS[target]} (${profile})`;
    logInfo(`Building ${label}...`);
    // Hand straight over to the Build panel: it shows the live output and a
    // running indicator, which is more use than a modal with a spinner. The
    // build itself is unaffected — this function keeps running after the
    // dialog it was called from is gone.
    beginBuild(`${label} → ${finalOutputDir}`);
    focusPanel("build");
    onclose();

    try {
      let output: string;
      if (target === "windows") {
        output = await buildWindows(
          proj.path,
          finalOutputDir,
          release,
          generateReport,
          !release && enableProfiling,
          debugOverlay,
        );
      } else if (target === "linux" || target === "macos") {
        output = await buildDesktop(
          target,
          proj.path,
          finalOutputDir,
          release,
          generateReport,
          debugOverlay,
        );
      } else if (target === "ios") {
        output = await buildIos(
          proj.path,
          finalOutputDir,
          release,
          debugOverlay,
          iosDevice,
          iosInstall,
          iosInstall && iosLaunch,
          iosDevice ? iosIdentity : null,
          iosDevice ? iosProfile : null,
        );
      } else if (target === "android") {
        const apk = await buildAndroid(
          proj.path,
          finalOutputDir,
          release,
          debugOverlay,
          androidInstall,
          androidInstall && androidLaunch,
          androidBundle,
          androidReleaseKey,
          androidReleaseKey ? androidKeystorePassword : null,
        );
        androidKeystorePassword = "";
        logInfo(`APK: ${apk}`);
        if (androidInstall) {
          logInfo("Its log: adb logcat -s jengine");
        }
        output = finalOutputDir;
      } else {
        output = await buildWeb(
          proj.path,
          finalOutputDir,
          release,
          generateReport,
          wasmLto,
          wasmOptSize,
          wasmSingleCodegen,
          wasmOptLevel,
          debugOverlay,
          webPlatform,
        );
      }
      logInfo(`${label} build complete: ${output}`);
      if (generateReport && target !== "android" && target !== "ios") {
        const reportPath = output.replace(/\\/g, "/") + "/build-report.html";
        try {
          await openBuildReport(reportPath);
        } catch {
          logInfo(`Build report saved to: ${reportPath}`);
        }
      }
      // Open the output folder in the file manager
      try {
        await openBuildFolder(output);
      } catch {
        // Non-fatal — just log if it fails
      }
      endBuild(`${label} build complete: ${output}`);
    } catch (e) {
      logError(`${label} build failed: ${e}`);
      endBuild(`${label} build failed: ${e}`);
      // A missing toolchain fails as a raw cargo error; say which tool it was.
      void checkToolsAfterBuildFailure(target);
    } finally {
      building = false;
    }
  }

  async function handleEstimateSavings() {
    const proj = get(project);
    if (!proj) {
      logError("Open a project first");
      return;
    }
    estimating = true;
    savingsResult = null;
    try {
      savingsResult = await estimateSavings(proj.path, compressionFormat);
      logInfo(
        `Estimate: ${savingsResult.file_count} files, ${savingsResult.total_savings_percent.toFixed(1)}% savings`,
      );
    } catch (e) {
      logError(`Estimate failed: ${e}`);
    } finally {
      estimating = false;
    }
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<!-- svelte-ignore a11y_click_events_have_key_events -->
<div class="overlay">
  <div
    class="dialog"
    onclick={(e) => e.stopPropagation()}
    role="dialog"
    tabindex="-1"
    aria-label="Build Settings"
  >
    <div class="dialog-header">
      <span>Build Settings</span>
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
        <!-- Shared build folder (applies to all platforms) -->
        <div class="shared-folder-section">
          <div class="field-row">
            <label>Build Folder</label>
            <input
              type="text"
              value={outputDir}
              placeholder="Choose a folder…"
              readonly
            />
            <button
              class="browse-btn"
              onclick={pickOutputDir}
              disabled={building}>Browse…</button
            >
          </div>
          {#if outputDir}
            <div class="path-preview">
              → {outputDir}/{activeCategory}/{profile}/
            </div>
          {:else}
            <div class="field-hint" style="margin-left: 108px; color: var(--ed-err);">
              No folder set — required before building
            </div>
          {/if}
          <div class="field-row" style="margin-top: 6px;">
            <label>Profile</label>
            <select bind:value={profile} disabled={building}>
              <option value="debug">Debug</option>
              <option value="release">Release</option>
            </select>
            <span class="field-hint">
              {#if profile === "debug"}
                Faster compile, debug logs, unoptimized
              {:else}
                Slower compile, optimized, logs stripped
              {/if}
            </span>
          </div>
        </div>

        {#if activeCategory === "windows"}
          <h3 class="content-title">Windows Build</h3>

          <hr class="section-divider" />

          <div class="field-row checkbox-row">
            <label>Build Report</label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                bind:checked={generateReport}
                disabled={building}
              />
              Generate HTML report with size analysis
            </label>
          </div>

          <div class="field-row checkbox-row">
            <label>Debug Overlay</label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                bind:checked={debugOverlay}
                disabled={building}
              />
              Let players open the F3 stats HUD / F12 inspector
            </label>
          </div>

          {#if profile === "debug"}
            <div class="field-row checkbox-row">
              <label>Script Profiling</label>
              <label class="checkbox-label">
                <input
                  type="checkbox"
                  bind:checked={enableProfiling}
                  disabled={building}
                />
                Emit <code>run-profiled.cmd</code> launcher with
                <code>[Script][PROF]</code> timing logs
              </label>
            </div>
          {/if}

          <hr class="section-divider" />
          <h4 class="section-subtitle">Asset Pipeline</h4>

          <div class="field-row checkbox-row">
            <label>Atlas Packing</label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                bind:checked={atlasEnabled}
                disabled={building}
              />
              Auto-pack sprites into atlas sheets
            </label>
          </div>

          {#if atlasEnabled}
            <div class="field-row">
              <label>Max Size</label>
              <select bind:value={atlasMaxSize} disabled={building}>
                <option value={1024}>1024</option>
                <option value={2048}>2048</option>
                <option value={4096}>4096</option>
              </select>
            </div>

            <div class="field-row">
              <label>Padding</label>
              <input
                type="number"
                bind:value={atlasPadding}
                min="0"
                max="16"
                disabled={building}
                style="width:60px"
              />
            </div>
          {/if}

          <div class="field-row checkbox-row">
            <label>Compression</label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                bind:checked={compressionEnabled}
                disabled={building}
              />
              Optimize textures during build
            </label>
          </div>

          {#if compressionEnabled}
            <div class="field-row">
              <label>Format</label>
              <select bind:value={compressionFormat} disabled={building}>
                <option value="png_optimized">Optimized PNG</option>
                <option value="webp_lossless">WebP (lossless)</option>
                <option value="webp_lossy">WebP (lossy)</option>
                <option value="basis_universal">Basis Universal (GPU)</option>
              </select>
            </div>

            <div class="field-row">
              <label></label>
              <button
                class="browse-btn"
                onclick={handleEstimateSavings}
                disabled={building || estimating}
              >
                {#if estimating}Estimating…{:else}Estimate Savings{/if}
              </button>
            </div>

            {#if savingsResult}
              <div class="savings-result">
                <span>{savingsResult.file_count} files</span>
                <span
                  >{(savingsResult.total_original_bytes / 1024).toFixed(0)} KB →
                  {(savingsResult.total_compressed_bytes / 1024).toFixed(0)} KB</span
                >
                <span class="savings-percent"
                  >−{savingsResult.total_savings_percent.toFixed(1)}%</span
                >
              </div>
            {/if}
          {/if}
        {:else if activeCategory === "linux" || activeCategory === "macos"}
          <h3 class="content-title">{TARGET_LABELS[activeCategory]} Build</h3>

          {#if activeCategory === "linux"}
            <p class="field-hint desktop-note">
              A folder: the game's executable, <code>project.jengine</code> and
              <code>assets/</code> beside it.
              {#if onWindows}
                Built inside <strong>WSL</strong> - it needs a Linux
                distribution with Rust and the development packages
                (<code>jengine linux-build</code> prints the apt line when they
                are missing). <code>~/.jengine/wsl-env.sh</code> there is sourced
                first, for a toolchain set up without root.
              {/if}
            </p>
          {:else}
            <p class="field-hint desktop-note">
              A <code>.app</code> bundle, its name, identifier, version and
              icon from Project Settings → Mobile App.
              {#if !onMac}
                <strong>It is built on a Mac</strong>: Apple's linker and SDK
                exist nowhere else. Open this project on a Mac, or run
                <code>jengine macos-build</code> there.
              {/if}
            </p>
          {/if}

          <hr class="section-divider" />

          <div class="field-row checkbox-row">
            <label>Build Report</label>
            <label class="checkbox-label">
              <input type="checkbox" bind:checked={generateReport} disabled={building} />
              Generate HTML report with size analysis
            </label>
          </div>

          <div class="field-row checkbox-row">
            <label>Debug Overlay</label>
            <label class="checkbox-label">
              <input type="checkbox" bind:checked={debugOverlay} disabled={building} />
              Let players open the F3 stats HUD / F12 inspector
            </label>
          </div>
        {:else if activeCategory === "ios"}
          <h3 class="content-title">iOS Build</h3>
          <p class="field-hint desktop-note">
            A <code>.app</code> with no Xcode project, its name, identifier,
            version, orientation and icon from Project Settings → Mobile App.
            {#if !onMac}
              <strong>It is built on a Mac</strong>: Apple's linker,
              <code>codesign</code> and the simulator exist nowhere else. Run
              <code>jengine ios-build</code> there.
            {/if}
          </p>

          <hr class="section-divider" />

          <div class="field-row">
            <label>Run On</label>
            <select bind:value={iosDevice} disabled={building}>
              <option value={false}>Simulator</option>
              <option value={true}>Device</option>
            </select>
          </div>

          <div class="field-row checkbox-row">
            <label>Install</label>
            <label class="checkbox-label">
              <input type="checkbox" bind:checked={iosInstall} disabled={building} />
              {iosDevice ? "Install on the connected device" : "Install on the simulator (booting one)"}
            </label>
          </div>

          {#if iosInstall}
            <div class="field-row checkbox-row">
              <label>Launch</label>
              <label class="checkbox-label">
                <input type="checkbox" bind:checked={iosLaunch} disabled={building} />
                Start it once installed
              </label>
            </div>
          {/if}

          {#if iosDevice}
            <div class="field-row">
              <label>Identity</label>
              <input
                type="text"
                bind:value={iosIdentity}
                placeholder="Apple Development: Name (TEAMID)"
                disabled={building}
              />
            </div>
            <div class="field-row">
              <label>Profile</label>
              <input type="text" value={iosProfile} placeholder="A .mobileprovision naming the app and device" readonly />
              <button class="browse-btn" onclick={pickIosProfile} disabled={building}>Browse…</button>
            </div>
          {/if}

          <div class="field-row checkbox-row">
            <label>Debug Overlay</label>
            <label class="checkbox-label">
              <input type="checkbox" bind:checked={debugOverlay} disabled={building} />
              Let players open the F3 stats HUD / F12 inspector
            </label>
          </div>
        {:else if activeCategory === "android"}
          <h3 class="content-title">Android Build</h3>
          <p class="field-hint" style="margin-bottom: 10px;">
            An APK signed with the debug key: it installs on any phone or
            emulator, and Google Play will not take it. The app id, name, icon,
            orientation and ABIs are in Project Settings → Mobile App. Needs the
            Android SDK, an NDK r25+ and <code>cargo install cargo-ndk</code>.
          </p>

          <div class="field-row checkbox-row">
            <label>Install</label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                bind:checked={androidInstall}
                disabled={building}
              />
              Install on the connected phone or emulator (adb)
            </label>
          </div>

          <div class="field-row checkbox-row">
            <label>Start</label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                bind:checked={androidLaunch}
                disabled={building || !androidInstall}
              />
              Start it once installed
            </label>
          </div>

          <div class="field-row checkbox-row">
            <label>Debug Overlay</label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                bind:checked={debugOverlay}
                disabled={building}
              />
              Ship the F3 stats HUD / F12 inspector (needs a keyboard)
            </label>
          </div>

          <hr class="section-divider" />
          <h4 class="section-subtitle">For Google Play</h4>

          <div class="field-row checkbox-row">
            <label>App Bundle</label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                bind:checked={androidBundle}
                disabled={building}
              />
              Also build the .aab Google Play takes (needs bundletool)
            </label>
          </div>

          <div class="field-row checkbox-row">
            <label>Release Key</label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                bind:checked={androidReleaseKey}
                disabled={building}
              />
              Sign with the keystore in Project Settings → Mobile App
            </label>
          </div>

          {#if androidReleaseKey}
            <div class="field-row">
              <label>Keystore Password</label>
              <input
                type="password"
                autocomplete="off"
                bind:value={androidKeystorePassword}
                disabled={building}
              />
              <span class="field-hint">Used for this build, never saved</span>
            </div>
          {/if}
        {:else if activeCategory === "web"}
          <h3 class="content-title">Web Build</h3>

          <div class="field-row">
            <label>Portal</label>
            <select bind:value={webPlatform} disabled={building}>
              {#each WEB_PLATFORMS as p}
                <option value={p.id}>{p.label}</option>
              {/each}
            </select>
            <span class="field-hint">
              Its SDK, loading bar, pause and ads (docs/web-platforms.md)
            </span>
          </div>

          <div class="field-row checkbox-row">
            <label>Build Report</label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                bind:checked={generateReport}
                disabled={building}
              />
              Generate HTML report with size analysis
            </label>
          </div>

          <div class="field-row checkbox-row">
            <label>Debug Overlay</label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                bind:checked={debugOverlay}
                disabled={building}
              />
              Let players open the F3 stats HUD / F12 inspector
            </label>
          </div>

          <hr class="section-divider" />
          <h4 class="section-subtitle">Asset Pipeline</h4>

          <div class="field-row checkbox-row">
            <label>Atlas Packing</label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                bind:checked={atlasEnabled}
                disabled={building}
              />
              Auto-pack sprites into atlas sheets
            </label>
          </div>

          {#if atlasEnabled}
            <div class="field-row">
              <label>Max Size</label>
              <select bind:value={atlasMaxSize} disabled={building}>
                <option value={1024}>1024</option>
                <option value={2048}>2048</option>
                <option value={4096}>4096</option>
              </select>
            </div>

            <div class="field-row">
              <label>Padding</label>
              <input
                type="number"
                bind:value={atlasPadding}
                min="0"
                max="16"
                disabled={building}
                style="width:60px"
              />
            </div>
          {/if}

          <div class="field-row checkbox-row">
            <label>Compression</label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                bind:checked={compressionEnabled}
                disabled={building}
              />
              Optimize textures during build
            </label>
          </div>

          {#if compressionEnabled}
            <div class="field-row">
              <label>Format</label>
              <select bind:value={compressionFormat} disabled={building}>
                <option value="png_optimized">Optimized PNG</option>
                <option value="webp_lossless">WebP (lossless)</option>
                <option value="webp_lossy">WebP (lossy)</option>
                <option value="basis_universal">Basis Universal (GPU)</option>
              </select>
            </div>

            <div class="field-row">
              <label></label>
              <button
                class="browse-btn"
                onclick={handleEstimateSavings}
                disabled={building || estimating}
              >
                {#if estimating}Estimating…{:else}Estimate Savings{/if}
              </button>
            </div>

            {#if savingsResult}
              <div class="savings-result">
                <span>{savingsResult.file_count} files</span>
                <span
                  >{(savingsResult.total_original_bytes / 1024).toFixed(0)} KB →
                  {(savingsResult.total_compressed_bytes / 1024).toFixed(0)} KB</span
                >
                <span class="savings-percent"
                  >−{savingsResult.total_savings_percent.toFixed(1)}%</span
                >
              </div>
            {/if}
          {/if}

          <hr class="section-divider" />
          <h4 class="section-subtitle">WASM Optimizations</h4>
          <div
            class="field-hint"
            style="margin-left: 108px; margin-bottom: 6px;"
          >
            These apply to <strong>release</strong> builds only. They increase build
            time but reduce WASM size.
          </div>

          <div class="field-row checkbox-row">
            <label>LTO</label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                bind:checked={wasmLto}
                disabled={building}
                onchange={() => updateSetting("wasm_lto", wasmLto)}
              />
              Enable link-time optimization
            </label>
          </div>

          <div class="field-row checkbox-row">
            <label>Opt for Size</label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                bind:checked={wasmOptSize}
                disabled={building}
                onchange={() => updateSetting("wasm_opt_size", wasmOptSize)}
              />
              Use <code>opt-level = 'z'</code> (size over speed)
            </label>
          </div>

          <div class="field-row checkbox-row">
            <label>Single Codegen</label>
            <label class="checkbox-label">
              <input
                type="checkbox"
                bind:checked={wasmSingleCodegen}
                disabled={building}
                onchange={() =>
                  updateSetting("wasm_single_codegen", wasmSingleCodegen)}
              />
              Use <code>codegen-units = 1</code> for better cross-crate optimization
            </label>
          </div>

          <div class="field-row">
            <label>wasm-opt</label>
            <select
              bind:value={wasmOptLevel}
              disabled={building}
              onchange={() => updateSetting("wasm_opt_level", wasmOptLevel)}
            >
              {#each WASM_OPT_LEVELS as level (level.id)}
                <option value={level.id}>{level.label}</option>
              {/each}
            </select>
          </div>
        {/if}
      </div>
    </div>

    <div class="dialog-footer">
      <button class="cancel-btn" onclick={onclose} disabled={building}
        >Close</button
      >
      <button
        class="build-btn"
        onclick={handleBuild}
        disabled={building || $buildRunning || ((activeCategory === "macos" || activeCategory === "ios") && !onMac)}
      >
        {#if building || $buildRunning}
          <span class="spinner"></span> Building…
        {:else}
          Build
        {/if}
      </button>
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
    font-family: inherit;
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

  .category-content {
    flex: 1;
    overflow-y: auto;
    padding: 16px;
  }

  .shared-folder-section {
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line);
    border-radius: 6px;
    padding: 10px 12px;
    margin-bottom: 14px;
  }

  .path-preview {
    margin-left: 108px;
    font-size: 11px;
    color: var(--ed-ok);
    font-family: monospace;
    margin-top: 2px;
    word-break: break-all;
  }

  .content-title {
    font-size: 14px;
    font-weight: 600;
    color: var(--ed-text);
    margin-bottom: 12px;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--ed-line);
  }

  .field-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 8px;
  }

  .field-row label {
    font-size: 12px;
    color: var(--ed-faint);
    min-width: 100px;
    flex-shrink: 0;
  }

  .field-hint {
    font-size: 11px;
    color: var(--ed-faint);
    margin-left: 4px;
  }

  input,
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
  }

  input:focus,
  select:focus {
    outline: none;
    border-color: var(--ed-accent);
  }

  select {
    cursor: pointer;
  }

  input[readonly] {
    cursor: default;
    color: var(--ed-muted);
  }

  .browse-btn {
    background: var(--ed-raised);
    border: 1px solid var(--ed-line-strong);
    border-radius: 4px;
    color: var(--ed-text);
    padding: 4px 10px;
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
    flex-shrink: 0;
    white-space: nowrap;
  }

  .browse-btn:hover:not(:disabled) {
    background: var(--ed-line);
  }

  .browse-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
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
    color: var(--ed-text);
    border-radius: 4px;
    padding: 6px 16px;
    font-size: 12px;
    font-family: inherit;
    cursor: pointer;
  }

  .cancel-btn:hover:not(:disabled) {
    background: var(--ed-line);
  }

  .cancel-btn:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .build-btn {
    background: var(--ed-accent);
    border: none;
    border-radius: 4px;
    color: var(--ed-on-accent);
    padding: 6px 24px;
    font-size: 12px;
    font-weight: 600;
    font-family: inherit;
    cursor: pointer;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .build-btn:hover:not(:disabled) {
    background: var(--ed-a2);
  }

  .build-btn:disabled {
    opacity: 0.7;
    cursor: not-allowed;
  }

  .spinner {
    display: inline-block;
    width: 10px;
    height: 10px;
    border: 2px solid color-mix(in srgb, var(--ed-base) 40%, transparent);
    border-top-color: var(--ed-base);
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .checkbox-row {
    margin-top: 8px;
  }

  .checkbox-label {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--ed-muted);
    cursor: pointer;
  }

  .checkbox-label input[type="checkbox"] {
    accent-color: var(--ed-accent);
    width: 14px;
    height: 14px;
    cursor: pointer;
    flex: none;
  }

  .section-divider {
    border: none;
    border-top: 1px solid var(--ed-line);
    margin: 12px 0;
  }

  .section-subtitle {
    font-size: 12px;
    font-weight: 600;
    color: var(--ed-muted);
    margin-bottom: 4px;
  }

  .savings-result {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-left: 108px;
    padding: 6px 10px;
    background: var(--ed-mantle);
    border: 1px solid var(--ed-line);
    border-radius: 4px;
    font-size: 12px;
    color: var(--ed-muted);
  }

  .savings-percent {
    color: var(--ed-ok);
    font-weight: 600;
  }
  .desktop-note {
    display: block;
    margin: 4px 0 8px;
    line-height: 1.5;
  }

  /* A number is a value: mono, as the design kit sets values. */
  input[type="number"] {
    font-family: var(--ed-mono);
  }
</style>
