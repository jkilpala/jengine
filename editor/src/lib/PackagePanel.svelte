<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import type { PackagePanelInfo } from "./stores/layoutStore";
  import { logInfo, logError, logWarn } from "./stores/consoleStore";
  import { scene } from "./stores/sceneStore";
  import { selectedEntity } from "./stores/selectionStore";
  import { project } from "./stores/projectStore";

  let { panelInfo }: { panelInfo: PackagePanelInfo } = $props();

  let iframeEl = $state<HTMLIFrameElement | null>(null);
  let loaded = $state(false);
  let error = $state<string | null>(null);

  // ──── Whitelisted Tauri commands that package panels may invoke ────
  const ALLOWED_COMMANDS = new Set([
    // Read-only project/scene queries
    "list_assets",
    "list_asset_tree",
    "read_asset_binary",
    "list_packages",
    "list_prefabs",
    // Scene entity queries (no mutations — panels shouldn't modify scene directly)
  ]);

  onMount(() => {
    loadPanelContent();

    function handleMessage(e: MessageEvent) {
      // Only accept messages from our iframe
      if (!iframeEl?.contentWindow || e.source !== iframeEl.contentWindow)
        return;

      const msg = e.data;
      if (!msg || typeof msg !== "object" || msg.type !== "jengine-panel-ipc")
        return;

      handleIpcMessage(msg);
    }

    window.addEventListener("message", handleMessage);
    return () => window.removeEventListener("message", handleMessage);
  });

  async function loadPanelContent() {
    try {
      const parts = panelInfo.url.split("|||");
      const packagePath = parts[0] || "";
      const bundlePath = parts.length > 1 ? parts[1] : "panel/index.html";

      const html = await invoke<string>("read_package_panel_html", {
        packagePath,
        webBundlePath: bundlePath,
      });

      // Inject IPC bridge script before </body> or at end
      const bridgeScript = generateBridgeScript();
      const injectedHtml = html.includes("</body>")
        ? html.replace("</body>", `${bridgeScript}</body>`)
        : html + bridgeScript;

      // Load via srcdoc for sandboxing
      if (iframeEl) {
        iframeEl.srcdoc = injectedHtml;
        loaded = true;
      }
    } catch (e) {
      error = `Failed to load panel: ${e}`;
      logError(`Package panel "${panelInfo.name}": ${error}`);
    }
  }

  function generateBridgeScript(): string {
    return `
<script>
  // jEngine Panel IPC Bridge — injected automatically
  window.jengine = {
    invoke: function(command, args) {
      return new Promise(function(resolve, reject) {
        var id = Math.random().toString(36).slice(2);
        function handler(e) {
          if (e.data && e.data.type === 'jengine-panel-response' && e.data.id === id) {
            window.removeEventListener('message', handler);
            if (e.data.error) reject(new Error(e.data.error));
            else resolve(e.data.result);
          }
        }
        window.addEventListener('message', handler);
        window.parent.postMessage({
          type: 'jengine-panel-ipc',
          action: 'invoke',
          id: id,
          command: command,
          args: args || {}
        }, '*');
      });
    },
    getScene: function() {
      return new Promise(function(resolve) {
        var id = Math.random().toString(36).slice(2);
        function handler(e) {
          if (e.data && e.data.type === 'jengine-panel-response' && e.data.id === id) {
            window.removeEventListener('message', handler);
            resolve(e.data.result);
          }
        }
        window.addEventListener('message', handler);
        window.parent.postMessage({
          type: 'jengine-panel-ipc',
          action: 'getScene',
          id: id
        }, '*');
      });
    },
    getSelectedEntity: function() {
      return new Promise(function(resolve) {
        var id = Math.random().toString(36).slice(2);
        function handler(e) {
          if (e.data && e.data.type === 'jengine-panel-response' && e.data.id === id) {
            window.removeEventListener('message', handler);
            resolve(e.data.result);
          }
        }
        window.addEventListener('message', handler);
        window.parent.postMessage({
          type: 'jengine-panel-ipc',
          action: 'getSelectedEntity',
          id: id
        }, '*');
      });
    },
    log: function(level, message) {
      window.parent.postMessage({
        type: 'jengine-panel-ipc',
        action: 'log',
        level: level,
        message: message
      }, '*');
    }
  };
<\/script>`;
  }

  async function handleIpcMessage(msg: Record<string, unknown>) {
    const action = msg.action as string;
    const id = msg.id as string;

    switch (action) {
      case "invoke": {
        const command = msg.command as string;
        const args = (msg.args as Record<string, unknown>) || {};

        if (!ALLOWED_COMMANDS.has(command)) {
          sendResponse(
            id,
            null,
            `Command "${command}" is not allowed for package panels`,
          );
          logWarn(
            `Package panel "${panelInfo.name}" tried to call blocked command: ${command}`,
          );
          return;
        }

        try {
          const result = await invoke(command, args);
          sendResponse(id, result, null);
        } catch (e) {
          sendResponse(id, null, String(e));
        }
        break;
      }

      case "getScene": {
        sendResponse(id, $scene, null);
        break;
      }

      case "getSelectedEntity": {
        sendResponse(id, $selectedEntity, null);
        break;
      }

      case "log": {
        const level = msg.level as string;
        const message = `[${panelInfo.name}] ${msg.message}`;
        if (level === "error") logError(message);
        else if (level === "warn") logWarn(message);
        else logInfo(message);
        break;
      }
    }
  }

  function sendResponse(id: string, result: unknown, error: string | null) {
    iframeEl?.contentWindow?.postMessage(
      { type: "jengine-panel-response", id, result, error },
      "*",
    );
  }
</script>

<div class="package-panel">
  {#if error}
    <div class="panel-error">
      <span class="error-icon">⚠</span>
      <span>{error}</span>
    </div>
  {:else if !loaded}
    <div class="panel-loading">Loading {panelInfo.name}…</div>
  {/if}
  <iframe
    bind:this={iframeEl}
    class="panel-iframe"
    class:hidden={!loaded || !!error}
    sandbox="allow-scripts"
    title="{panelInfo.name} panel"
  ></iframe>
</div>

<style>
  .package-panel {
    width: 100%;
    height: 100%;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--ed-base);
  }

  .panel-iframe {
    flex: 1;
    border: none;
    width: 100%;
    height: 100%;
    background: var(--ed-base);
  }

  .panel-iframe.hidden {
    display: none;
  }

  .panel-loading {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    color: var(--ed-faint);
    font-size: 12px;
  }

  .panel-error {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    height: 100%;
    color: var(--ed-err);
    font-size: 12px;
    padding: 12px;
    text-align: center;
  }

  .error-icon {
    font-size: 16px;
  }
</style>
