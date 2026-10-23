<script lang="ts">
  import { onDestroy } from "svelte";
  import { project } from "./stores/projectStore";
  import {
    localLog,
    fleetLog,
    appendServerLine,
    clearServerLog,
    formatUptime,
    latencyGrade,
    roomRows,
    serviceAddress,
    type ServerLine,
  } from "./stores/serverStore";
  import {
    serverConfig,
    serverStart,
    serverStop,
    serverStatus,
    serverNotice,
    serverOpenDashboard,
    fleetCreate,
    fleetStatus,
    fleetDeploy,
    fleetAct,
    fleetLogs,
    fleetLocalAgent,
    fleetOpenConsole,
    type ServerConfig,
    type LocalStatus,
    type FleetStatus,
    type Probe,
  } from "./api/servers";

  /**
   * The project's test servers: the one running here (start, stop, watch
   * its rooms and log) and the deployed ones (deploy through the fleet,
   * start / stop / restart / roll back, watch how each answers). Everything
   * is `jengine_builder::servers` behind the `server_*` / `fleet_*` commands.
   */
  let tab = $state<"local" | "deployed">("local");
  let cfg = $state<ServerConfig | null>(null);
  let cfgError = $state<string | null>(null);
  let local = $state<LocalStatus | null>(null);
  let fleet = $state<FleetStatus | null>(null);
  let fleetError = $state<string | null>(null);
  let busy = $state<string | null>(null);
  let notice = $state("");
  let listEl: HTMLDivElement | undefined = $state();
  let pinned = $state(true);

  const path = $derived($project?.path ?? null);
  const lines = $derived(tab === "local" ? $localLog : $fleetLog);
  const rooms = $derived(roomRows(local?.probe?.overview));

  async function loadConfig() {
    cfg = null;
    cfgError = null;
    if (!path) return;
    try {
      cfg = await serverConfig(path);
    } catch (e) {
      cfgError = String(e);
    }
  }

  $effect(() => {
    void path;
    loadConfig();
  });

  // Polling: the local server every 2 s, the fleet every 5 s while its tab
  // is open. A poll never overlaps the one before (an unreachable machine
  // takes its whole timeout to say so).
  let polling = false;
  let fleetPolling = false;
  async function pollLocal() {
    if (polling || !path || !cfg) return;
    polling = true;
    try {
      local = await serverStatus(path);
      if (local.exited) appendServerLine("local", `== the server stopped by itself: ${local.exited}`);
    } catch (e) {
      local = null;
    } finally {
      polling = false;
    }
  }
  async function pollFleet() {
    if (fleetPolling || !path || !cfg?.fleet_exists || tab !== "deployed") return;
    fleetPolling = true;
    try {
      fleet = await fleetStatus(path);
      fleetError = null;
    } catch (e) {
      fleetError = String(e);
    } finally {
      fleetPolling = false;
    }
  }
  const localTimer = setInterval(pollLocal, 2000);
  const fleetTimer = setInterval(pollFleet, 5000);
  onDestroy(() => {
    clearInterval(localTimer);
    clearInterval(fleetTimer);
  });
  $effect(() => {
    void cfg;
    pollLocal();
  });
  $effect(() => {
    if (tab === "deployed") pollFleet();
  });

  /** Run one action at a time, its failure in the log rather than lost. */
  async function run(label: string, source: "local" | "fleet", action: () => Promise<unknown>) {
    if (busy) return;
    busy = label;
    try {
      await action();
    } catch (e) {
      appendServerLine(source, `== ${label} failed: ${e}`);
    } finally {
      busy = null;
      pollLocal();
      pollFleet();
    }
  }

  const start = () =>
    run("start", "local", async () => {
      appendServerLine("local", `== building and starting ${cfg?.bin} ...`);
      await serverStart(path!);
    });
  const stop = () => run("stop", "local", () => serverStop());
  const restart = () =>
    run("restart", "local", async () => {
      await serverStop();
      appendServerLine("local", `== building and starting ${cfg?.bin} ...`);
      await serverStart(path!);
    });
  const sendNotice = () => {
    const text = notice.trim();
    if (!text) return;
    run("notice", "local", async () => {
      await serverNotice(path!, text);
      appendServerLine("local", `== notice sent: ${text}`);
      notice = "";
    });
  };

  const openDashboard = (address: string | null = null) =>
    run("open dashboard", "local", () => serverOpenDashboard(path!, address));

  const createFleet = () =>
    run("create fleet file", "fleet", async () => {
      const file = await fleetCreate(path!);
      appendServerLine("fleet", `== wrote ${file}`);
      await loadConfig();
    });
  const deploy = (machine: string | null) =>
    run("deploy", "fleet", async () => {
      appendServerLine("fleet", `== deploying ${cfg?.service}${machine ? ` to ${machine}` : ""} ...`);
      await fleetDeploy(path!, machine);
    });
  const act = (action: string, machine: string | null = null) =>
    run(action, "fleet", () => fleetAct(path!, action, machine));
  const logs = (machine: string) =>
    run("logs", "fleet", async () => {
      const text = await fleetLogs(path!, machine);
      appendServerLine("fleet", `== the last lines on ${machine}:`);
      for (const line of text.split(/\r?\n/)) if (line) appendServerLine("fleet", line);
    });
  const toggleAgent = (on: boolean) =>
    run(on ? "start test machine" : "stop test machine", "fleet", () => fleetLocalAgent(path!, on));

  function onScroll() {
    if (!listEl) return;
    pinned = listEl.scrollHeight - listEl.scrollTop - listEl.clientHeight < 24;
  }
  $effect(() => {
    void lines.length;
    if (!pinned || !listEl) return;
    requestAnimationFrame(() => {
      if (listEl && pinned) listEl.scrollTop = listEl.scrollHeight;
    });
  });

  function color(kind: ServerLine["kind"]) {
    return { plain: "var(--ed-text)", error: "var(--ed-err)", warning: "var(--ed-warn)", note: "var(--ed-a2)" }[kind];
  }
  function dot(p: Probe | null | undefined, running = true) {
    if (!running) return "off";
    if (!p?.up) return p?.ms != null ? "fair" : "down";
    return latencyGrade(p.ms);
  }
  const ms = (p: Probe | null | undefined) => (p?.ms != null ? `${p.ms.toFixed(1)} ms` : "-");
  const shortVersion = (v: string | null | undefined) => (v ? v.slice(0, 10) : "-");
</script>

<div class="servers">
  <div class="header">
    <span class="title">
      Servers
      {#if cfg}<span class="sub">{cfg.bin} · {cfg.service}</span>{/if}
      {#if busy}<span class="busy">● {busy}…</span>{/if}
    </span>
    <span class="tabs">
      <button class:active={tab === "local"} onclick={() => (tab = "local")}>Local</button>
      <button class:active={tab === "deployed"} onclick={() => (tab = "deployed")}>Deployed</button>
      <button class="quiet" onclick={() => clearServerLog(tab === "local" ? "local" : "fleet")}>Clear log</button>
    </span>
  </div>

  {#if !path}
    <div class="empty">Open a project to run its server.</div>
  {:else if cfgError}
    <div class="empty error">{cfgError}</div>
  {:else if !cfg}
    <div class="empty">
      This project names no server. Add a <code>"server"</code> block to <code>project.jengine</code>:
      <pre>"server": &#123; "package": "my-game-server", "address": "127.0.0.1:9001",
            "args": ["&#123;address&#125;", "game.db"], "fleet": "server/fleet.json" &#125;</pre>
      <code>package</code> is a crate in the engine workspace; the editor builds it, runs it, and deploys it.
    </div>
  {:else}
    <div class="body">
      <div class="side">
        {#if tab === "local"}
          <div class="status">
            <span class="dot {dot(local?.probe, local?.running || local?.probe?.up)}"></span>
            {#if local?.running}
              running · pid {local.pid} · up {formatUptime(local.uptime)}
            {:else if local?.probe?.up}
              something else is answering on {cfg.address}
            {:else}
              stopped
            {/if}
          </div>
          <div class="facts">
            <span>{cfg.address}</span>
            <span>health {ms(local?.probe)}</span>
            {#if local?.probe?.overview}
              <span>{local.probe.overview.players} online</span>
              <span>{local.probe.overview.connections} connections</span>
              <span>{rooms.length} rooms</span>
            {/if}
          </div>
          <div class="buttons">
            <button disabled={!!busy || local?.running} onclick={start}>Build &amp; start</button>
            <button disabled={!!busy || !local?.running} onclick={restart}>Restart</button>
            <button disabled={!!busy || !local?.running} onclick={stop}>Stop</button>
          </div>
          <div class="notice">
            <input
              placeholder="notice to everyone on it"
              bind:value={notice}
              onkeydown={(e) => e.key === "Enter" && sendNotice()}
              disabled={!local?.probe?.overview}
            />
            <button disabled={!!busy || !notice.trim() || !local?.probe?.overview} onclick={sendNotice}>Send</button>
          </div>
          {#if local?.probe?.overview}
            {#if local.probe.overview.warnings?.length}
              {#each local.probe.overview.warnings as w}<div class="warn">⚠ {w}</div>{/each}
            {/if}
            <table>
              <thead><tr><th>room</th><th>players</th><th>tick</th><th>cpu</th><th>up</th></tr></thead>
              <tbody>
                {#each rooms as r}
                  <tr>
                    <td>{r.kind}/{r.id}</td>
                    <td>{r.players}</td>
                    <td>{r.tickMs != null ? `${r.tickMs.toFixed(2)} ms` : "-"}</td>
                    <td>{r.cpu != null ? `${r.cpu.toFixed(1)}%` : "-"}</td>
                    <td>{formatUptime(r.uptime)}</td>
                  </tr>
                {/each}
                {#if rooms.length === 0}<tr><td colspan="5" class="none">no rooms open yet</td></tr>{/if}
              </tbody>
            </table>
            <div class="hint">
              <button class="link" onclick={() => openDashboard()} title="Opens in your browser, signed in with the project's admin token">
                Open dashboard ↗
              </button>
              <span class="dim">{cfg.admin_url}</span>
            </div>
          {:else if local?.probe?.error && local?.running}
            <div class="hint">{local.probe.error}</div>
          {/if}
        {:else}
          {#if !cfg.fleet}
            <div class="hint">The server block names no <code>"fleet"</code> file to deploy through.</div>
          {:else if !cfg.fleet_exists}
            <div class="hint">No fleet file at {cfg.fleet} yet.</div>
            <div class="buttons">
              <button disabled={!!busy} onclick={createFleet}>Create fleet file</button>
            </div>
            <div class="hint">It deploys to a test machine on this computer; add real machines to it later.</div>
          {:else}
            <div class="buttons">
              <span class="label">Test machine here:</span>
              {#if fleet?.local_agent}
                <span class="ok">{fleet.local_agent}</span>
                <button disabled={!!busy} onclick={() => toggleAgent(false)}>Stop</button>
              {:else}
                <button disabled={!!busy} onclick={() => toggleAgent(true)}>Start</button>
              {/if}
            </div>
            <div class="buttons">
              <button disabled={!!busy} onclick={() => deploy(null)}>Build &amp; deploy all</button>
              <button disabled={!!busy} onclick={() => act("restart")}>Restart</button>
              <button disabled={!!busy} onclick={() => act("stop")}>Stop</button>
              <button disabled={!!busy} onclick={() => act("start")}>Start</button>
              <button disabled={!!busy} onclick={() => act("rollback")}>Roll back</button>
            </div>
            {#if fleetError}<div class="warn">{fleetError}</div>{/if}
            {#each fleet?.machines ?? [] as m}
              <div class="machine">
                <div class="mrow">
                  <span class="dot {m.error ? 'down' : 'good'}"></span>
                  <b>{m.name}</b>
                  <span class="dim">{m.url}</span>
                  {#if m.info}<span class="dim">{m.info.platform}</span>{/if}
                  <span class="grow"></span>
                  <button class="small" disabled={!!busy} onclick={() => deploy(m.name)}>Deploy here</button>
                  <button class="small" disabled={!!busy || !!m.error} onclick={() => logs(m.name)}>Logs</button>
                  {#if m.console}
                    <button class="small" title="The host's full management page, in your browser"
                      onclick={() => run("open console", "fleet", () => fleetOpenConsole(path!, m.name))}>Console ↗</button>
                  {/if}
                </div>
                {#if m.error}
                  <div class="warn">{m.error}</div>
                {:else}
                  {#each m.info?.services ?? [] as s}
                    <div class="svc">
                      <span class="dot {s.status === 'running' ? dot(s.probe) : 'off'}"></span>
                      {s.name}: {s.status}
                      <span class="dim">v {shortVersion(s.version)}</span>
                      {#if s.port}<span class="dim">:{s.port}</span>{/if}
                      <span class="dim">up {formatUptime(s.uptime)}</span>
                      {#if s.restarts}<span class="warnText">{s.restarts} restarts</span>{/if}
                      {#if s.probe}
                        <span class="dim">health {ms(s.probe)}</span>
                        {#if s.probe.overview}
                          <span class="dim">{s.probe.overview.players} online</span>
                          {#if s.port}
                            <button class="link" onclick={() => openDashboard(serviceAddress(m.url, s.port!))}>dashboard ↗</button>
                          {/if}
                        {/if}
                      {/if}
                      {#if s.last_error}<div class="warn">{s.last_error}</div>{/if}
                    </div>
                  {/each}
                  {#if (m.info?.services ?? []).length === 0}<div class="hint">nothing deployed here yet</div>{/if}
                {/if}
              </div>
            {/each}
          {/if}
        {/if}
      </div>

      <div class="log" bind:this={listEl} onscroll={onScroll}>
        {#each lines as line}
          <div class="line" style="color: {color(line.kind)}">{line.text}</div>
        {/each}
        {#if lines.length === 0}
          <div class="none">{tab === "local" ? "The local server's output appears here." : "Builds, deploys and fetched logs appear here."}</div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .servers { display: flex; flex-direction: column; height: 100%; background: var(--ed-base); color: var(--ed-text); font-size: 12px; }
  .header { display: flex; align-items: center; justify-content: space-between; padding: 4px 10px; background: var(--ed-mantle); border-top: 1px solid var(--ed-line); font-size: 11px; }
  .title { font-weight: 600; text-transform: uppercase; color: var(--ed-muted); }
  .sub { text-transform: none; font-weight: 400; color: var(--ed-faint); margin-left: 6px; }
  .busy { text-transform: none; color: var(--ed-ok); margin-left: 8px; font-weight: 500; }
  .tabs { display: flex; gap: 2px; }
  .tabs button { background: none; border: none; color: var(--ed-faint); cursor: pointer; font-size: 11.5px; padding: 3px 8px; border-radius: 4px; }
  .tabs button.active { color: var(--ed-text); background: var(--ed-raised); }
  .tabs button:hover { color: var(--ed-text); }
  .body { flex: 1; display: flex; min-height: 0; }
  .side { width: 46%; min-width: 320px; overflow-y: auto; padding: 6px 10px; border-right: 1px solid var(--ed-line); display: flex; flex-direction: column; gap: 6px; }
  .log { flex: 1; overflow-y: auto; padding: 4px 0; font-family: "Cascadia Code", "Fira Code", monospace; user-select: text; -webkit-user-select: text; }
  .line { padding: 0 10px; line-height: 1.45; white-space: pre-wrap; word-break: break-word; }
  .line:hover { background: var(--ed-hover); }
  .status { display: flex; align-items: center; gap: 6px; font-weight: 600; }
  .facts { display: flex; flex-wrap: wrap; gap: 10px; color: var(--ed-muted); }
  .buttons { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; }
  button { background: var(--ed-raised); border: 1px solid var(--ed-line-strong); color: var(--ed-text); border-radius: 4px; padding: 3px 10px; cursor: pointer; font-size: 12px; }
  button:hover:not(:disabled) { background: var(--ed-line); }
  button:disabled { opacity: 0.45; cursor: default; }
  button.small { padding: 3px 6px; font-size: 11.5px; }
  button.link { background: none; border: none; padding: 0; color: var(--ed-accent); text-decoration: underline; cursor: pointer; }
  button.link:hover:not(:disabled) { background: none; color: var(--ed-accent-high); }
  .hint button.link { margin-right: 8px; }
  .notice { display: flex; gap: 4px; }
  .notice input { flex: 1; background: var(--ed-crust); border: 1px solid var(--ed-line-strong); color: var(--ed-text); border-radius: 4px; padding: 3px 6px; font-size: 12px; }
  table { border-collapse: collapse; width: 100%; }
  th { text-align: left; color: var(--ed-faint); font-weight: 500; padding: 1px 6px 1px 0; }
  td { padding: 1px 6px 1px 0; font-variant-numeric: tabular-nums; }
  .none, .empty { color: var(--ed-faint); font-style: italic; padding: 4px 10px; }
  .empty { padding: 10px; font-style: normal; }
  .empty pre { background: var(--ed-crust); padding: 6px; border-radius: 4px; color: var(--ed-muted); }
  .hint { color: var(--ed-faint); }
  .warn, .error { color: var(--ed-err); }
  .warnText { color: var(--ed-warn); }
  .ok { color: var(--ed-ok); }
  .dim { color: var(--ed-faint); }
  .label { color: var(--ed-muted); }
  .grow { flex: 1; }
  .machine { border: 1px solid var(--ed-line); border-radius: 4px; padding: 4px 6px; }
  .mrow, .svc { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; }
  .svc { padding-left: 14px; }
  .dot { width: 8px; height: 8px; border-radius: 50%; display: inline-block; flex: none; }
  .dot.good { background: var(--ed-ok); }
  .dot.fair { background: var(--ed-warn); }
  .dot.poor { background: var(--ed-a3); }
  .dot.down { background: var(--ed-err); }
  .dot.off { background: var(--ed-line-strong); }
  code { color: var(--ed-accent-high); }
</style>
