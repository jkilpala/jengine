// Click through the web pages in a real browser - the server's admin
// dashboard, its live ops page and the fleet's web interface - and check
// what each click did.
//
// The pages' APIs are tested from Rust; this is the other half: that a button
// on the page reaches its API and the page then shows the result. It drives a
// headless Edge or Chrome over the DevTools protocol, with nothing but Node
// (22 or later, for its built-in WebSocket).
//
//   node click_pages.mjs --admin http://127.0.0.1:9921/admin --admin-token <t> \
//                        --fleet http://127.0.0.1:9800 --service zones --machine us-1
//   node click_pages.mjs --liveops http://127.0.0.1:9921/admin/liveops --admin-token <t>
//   node click_pages.mjs --console http://127.0.0.1:9012/manage --console-token <t> //                        --build target/debug/jengine-server.exe --port 9931
//
// The console run wants a fresh host (`jengine-fleet host` with an empty
// --data): it makes a service from an uploaded build, deploys it, opens its
// dashboard and uploads the build again from the service's card.
//
// The live ops run wants a server started with `--liveops <dir>` and fresh
// channels: it adds a parameter, a rule, an experiment and an event to `dev`,
// previews them, and promotes the config to `staging`.
//
// It expects them running and busy: a `zones` server with players in it (the
// `bots` example), reachable by the fleet UI as --service on --machine, and
// that machine's build "behind" the local one if --expect-behind is given.
// Everything it does it undoes or is harmless: a coin, a notice, a kick of a
// bot, a drain switched on and off, a stop and a start.
//
// Exit code 0 when every step did what it should; otherwise the failing step.

import { spawn } from "node:child_process";
import { existsSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, word, i, all) => {
    if (word.startsWith("--")) pairs.push([word.slice(2), all[i + 1]?.startsWith("--") || all[i + 1] === undefined ? true : all[i + 1]]);
    return pairs;
  }, []),
);

const browsers = [
  process.env.BROWSER,
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "/usr/bin/google-chrome", "/usr/bin/chromium", "/usr/bin/chromium-browser",
].filter(Boolean);
const browser = browsers.find(existsSync);
if (!browser) throw new Error("no Edge or Chrome found; set BROWSER to one");

const port = 9300 + Math.floor(Math.random() * 500);
const child = spawn(browser, [
  "--headless=new", "--disable-gpu", `--remote-debugging-port=${port}`,
  `--user-data-dir=${mkdtempSync(join(tmpdir(), "click-pages-"))}`, "about:blank",
], { stdio: "ignore" });

const sleep = ms => new Promise(r => setTimeout(r, ms));

async function devtools(path, method = "GET") {
  for (let i = 0; i < 50; i++) {
    try { return await (await fetch(`http://127.0.0.1:${port}${path}`, { method })).json(); }
    catch { await sleep(200); }
  }
  throw new Error("the browser's DevTools port never answered");
}

// One tab, spoken to over the DevTools protocol.
async function open(url) {
  const target = await devtools(`/json/new?${encodeURIComponent(url)}`, "PUT");
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((ok, no) => { ws.onopen = ok; ws.onerror = no; });
  let next = 0;
  const waiting = new Map();
  // A file dialog the page opens is answered with the file put up next.
  let nextFile = null;
  ws.onmessage = m => {
    const msg = JSON.parse(m.data);
    if (waiting.has(msg.id)) { waiting.get(msg.id)(msg); waiting.delete(msg.id); }
    if (msg.method === "Page.fileChooserOpened" && nextFile) {
      send("DOM.setFileInputFiles", { files: [nextFile], backendNodeId: msg.params.backendNodeId });
      nextFile = null;
    }
  };
  const send = (method, params = {}) => new Promise(done => {
    const id = ++next;
    waiting.set(id, done);
    ws.send(JSON.stringify({ id, method, params }));
  });
  const run = async js => {
    // A page asks before it does anything drastic; the answer here is yes.
    // Set on every call, since a navigation gives the page a new `window`.
    const yes = "window.confirm = () => true; window.alert = m => { window.__alert = m; }; window.prompt = () => window.__answer ?? null;";
    // As a click a person made: a file dialog opens only for one.
    const r = await send("Runtime.evaluate", { expression: `(async () => { ${yes} ${js} })()`, awaitPromise: true, returnByValue: true, userGesture: true });
    if (r.result?.exceptionDetails) throw new Error(r.result.exceptionDetails.exception?.description || "the page threw");
    return r.result?.result?.value;
  };
  return {
    run,
    // Wait for the page to show something, polling as a person would look.
    async until(what, js, seconds = 20) {
      const deadline = Date.now() + seconds * 1000;
      for (;;) {
        const got = await run(`return ${js};`);
        if (got) return got;
        if (Date.now() > deadline) {
          const alerted = await run("return window.__alert || ''");
          throw new Error(`timed out waiting for: ${what}${alerted ? ` (the page alerted: ${alerted})` : ""}`);
        }
        await sleep(150);
      }
    },
    // Click the first button (or link) with this text inside a selector.
    async click(within, text) {
      const hit = await run(`
        const scope = document.querySelector(${JSON.stringify(within)});
        const el = scope && [...scope.querySelectorAll("button, a.btn, tr.pick")]
          .find(b => !b.disabled && b.textContent.trim().startsWith(${JSON.stringify(text)}));
        if (el) el.click();
        return !!el;`);
      if (!hit) throw new Error(`no enabled "${text}" in ${within}`);
    },
    // The file a page's own file input (by selector) holds.
    async setFile(selector, path) {
      const { result: { root } } = await send("DOM.getDocument");
      const { result: { nodeId } } = await send("DOM.querySelector", { nodeId: root.nodeId, selector });
      if (!nodeId) throw new Error("no " + selector);
      await send("DOM.setFileInputFiles", { files: [path], nodeId });
    },
    // The file the next dialog the page opens is given.
    async putUpFile(path) {
      await send("Page.enable");
      await send("Page.setInterceptFileChooserDialog", { enabled: true });
      nextFile = path;
    },
    close: () => ws.close(),
  };
}

let step = "starting";
const did = [];
const say = line => { step = line; did.push(line); console.log("  ✓ " + line); };

async function adminDashboard() {
  console.log("admin dashboard: " + args.admin);
  const page = await open(`${args.admin}#token=${encodeURIComponent(args["admin-token"])}`);
  await page.until("the overview to load", `document.querySelectorAll("#tiles .tile").length >= 8 && !document.getElementById("status").classList.contains("bad")`);
  say("the overview loads with the token from the address");
  await page.until("a room in the table", `document.querySelectorAll("#rooms tr.pick").length > 0`);
  await page.until("players in the table", `document.querySelectorAll("#players tbody tr").length > 0`);
  say("rooms and players are listed");

  // The first room, whatever its columns are: in a cluster a row begins
  // with its node.
  await page.run(`document.querySelector("#rooms tr.pick").click();`);
  await page.until("the room's page", `!document.getElementById("detail").hidden && document.querySelectorAll("#detailPlayers tbody tr").length > 0`);
  const state = await page.run(`return document.querySelector("#detailPlayers tbody tr td:nth-child(3)").textContent;`);
  if (!state.includes('"zone"')) throw new Error("a player's live state is not shown: " + state);
  const info = await page.run(`return document.getElementById("detailInfo").textContent;`);
  if (!info.includes("entities")) throw new Error("the room's own description is not shown: " + info);
  say("clicking a room opens it: players with live state, and what the room says of itself");

  await page.run(`document.getElementById("cmd").value = '{"coin":[500,300]}';`);
  await page.click("#detail", "Send command");
  await page.until("the room's answer", `document.getElementById("cmdOut").textContent.includes('"ok": true')`);
  say("a room command is sent and its answer shown");

  await page.run(`document.getElementById("notice").value = "clicked from a browser";`);
  await page.run(`document.getElementById("noticeBtn").click();`);
  await page.until("the notice in the event log", `document.getElementById("events").textContent.includes("clicked from a browser")`);
  say("a notice is sent and recorded");

  const before = await page.run(`return document.querySelectorAll("#detailPlayers tbody tr").length;`);
  const kicked = await page.run(`return document.querySelector("#detailPlayers tbody tr td:nth-child(2)").textContent;`);
  await page.click("#detailPlayers", "Kick");
  await page.until("the kick in the event log", `document.getElementById("events").textContent.includes("kicked ${kicked}")`);
  await page.until("one player fewer in the room", `document.querySelectorAll("#detailPlayers tbody tr").length < ${before}`);
  say("Kick puts a player out of the room");

  await page.click("header", "Drain");
  await page.until("the draining badge", `!document.getElementById("drainBadge").hidden`);
  await page.click("header", "Stop draining");
  await page.until("the badge to go", `document.getElementById("drainBadge").hidden`);
  say("Drain switches on and off");

  await page.click("#detail", "Hide");
  await page.until("the room's page to close", `document.getElementById("detail").hidden`);
  say("Hide closes the room's page");

  await page.run(`const r = document.getElementById("historyRange"); r.value = "days"; r.dispatchEvent(new Event("change"));`);
  await page.until("the thirty-day range", `document.getElementById("historyNote").textContent.includes("thirty days")`);
  await page.run(`const r = document.getElementById("historyRange"); r.value = "hours"; r.dispatchEvent(new Event("change"));`);
  await page.until("the six-hour range back", `document.getElementById("historyNote").textContent.includes("six hours")`);
  say("the history switches between six hours and thirty days");
  page.close();
}

async function liveopsPage() {
  console.log("live ops page: " + args.liveops);
  const page = await open(`${args.liveops}#token=${encodeURIComponent(args["admin-token"])}`);
  const status = `document.getElementById("status").textContent`;
  await page.until("the channels", `document.querySelectorAll("#tabs button").length >= 3 && ${status}.startsWith("loaded dev")`);
  await page.until("the public key", `document.getElementById("keyRow").textContent.includes("jlo-public-")`);
  say("the page loads the dev channel and shows the key games are built with");

  await page.run(`window.__answer = "click_coins";`);
  await page.click("main", "+ Parameter");
  await page.until("the new parameter", `[...document.querySelectorAll("#params td.mono")].some(td => td.textContent === "click_coins")`);
  await page.run(`const i = document.querySelector("#params tbody tr input.mono"); i.value = "25"; i.dispatchEvent(new Event("change"));`);
  await page.until("Save to light up", `!document.getElementById("saveBtn").disabled`);
  await page.run(`document.querySelector("#params details").open = true;`);
  await page.click("#params", "+ Rule");
  await page.run(`
    const card = document.querySelector("#params .card");
    const value = card.querySelector("input.mono"); value.value = "40"; value.dispatchEvent(new Event("change"));
    const plat = card.querySelector(".cond input"); plat.value = "android"; plat.dispatchEvent(new Event("change"));`);
  await page.click("main", "+ Experiment");
  await page.run(`const s = document.querySelector("#experiments select"); s.value = "running"; s.dispatchEvent(new Event("change"));`);
  await page.run(`const v = [...document.querySelectorAll("#experiments textarea")][1]; v.value = '{"click_coins": 30}'; v.dispatchEvent(new Event("change"));`);
  await page.click("main", "+ Event");
  await page.until("the event on the timeline", `document.querySelectorAll("#timeline rect").length === 1`);
  await page.run(`document.getElementById("saveBtn").click();`);
  await page.until("revision 1", `document.getElementById("revision").textContent.startsWith("revision 1")`);
  say("a parameter, a rule, a running experiment and an event are added and saved");

  await page.run(`document.getElementById("pvPlatform").value = "android"; document.getElementById("pvInstall").value = "click-install";`);
  await page.click("main", "Preview");
  await page.until("the preview", `document.getElementById("pvOut").textContent.includes("click_coins")`);
  const variant = await page.run(`return [...document.querySelectorAll("#pvOut .pill.on")].map(p => p.textContent).join(",");`);
  if (!variant.includes("experiment_1 = ")) throw new Error("the preview shows no variant: " + variant);
  const coins = await page.run(`return [...document.querySelectorAll("#pvOut tr")].find(r => r.children[0].textContent === "click_coins").children[1].textContent;`);
  const expected = variant.includes("experiment_1 = test") ? "30" : "40";
  if (coins !== expected) throw new Error(`an android player in ${variant} should get ${expected}, the preview says ${coins}`);
  say("the preview shows what an android player in their variant is told");

  await page.run(`document.getElementById("pConfig").checked = true; document.getElementById("promoteTo").value = "staging";`);
  await page.click("main", "Promote");
  await page.until("the promotion", `${status}.startsWith("loaded dev")`);
  await page.click("#tabs", "staging");
  await page.until("staging", `${status}.startsWith("loaded staging")`);
  await page.until("the promoted parameter", `[...document.querySelectorAll("#params td.mono")].some(td => td.textContent === "click_coins")`);
  if (await page.run(`return document.querySelectorAll("#events .card").length;`) !== 0) throw new Error("events were promoted though not asked for");
  say("Promote copies dev's config to staging, and only the config");
  page.close();
}

async function fleetInterface() {
  console.log("fleet web interface: " + args.fleet);
  const { service, machine } = args;
  const page = await open(args.fleet);
  // The row for this service on this machine, and things inside it.
  const row = `[...document.querySelectorAll("#machines tbody tr")].find(r => r.children[0].textContent === ${JSON.stringify(machine)} && r.children[2].textContent === ${JSON.stringify(service)})`;
  const status = `(${row})?.children[4].textContent`;
  const clickInRow = async text => {
    const hit = await page.run(`const b = [...(${row}).querySelectorAll("button")].find(b => !b.disabled && b.textContent === ${JSON.stringify(text)}); if (b) b.click(); return !!b;`);
    if (!hit) throw new Error(`no enabled "${text}" on ${machine}`);
  };
  const jobDone = title => page.until(`the job "${title}" to finish`,
    `[...document.querySelectorAll("#jobs .job")].some(j => j.querySelector("h4").textContent.includes(${JSON.stringify(title)}) && j.querySelector(".badge").textContent === "done")`, 60);
  const idle = () => page.until("the buttons to come back", `!(${row}).querySelector("button:disabled:not([title])")`, 30);

  await page.until("the machines table", `${status} === "running"`);
  await page.until("a service card", `document.querySelectorAll("#services .card").length > 0`);
  say("machines, services and their status are listed, with no sign-in on loopback");

  await clickInRow("Logs");
  await page.until("the log", `!document.getElementById("logs").hidden && document.getElementById("logText").textContent.includes("listening on")`);
  await page.click("#logs", "Hide");
  say("Logs shows the server's output");

  if (args["expect-behind"]) {
    await page.until("the behind badge", `(${row}).querySelector(".badge.behind")`);
    await clickInRow("Update");
    await jobDone(`deploy ${service} to ${machine}`);
    await page.until("the badge to go", `${status} === "running" && !(${row}).querySelector(".badge.behind")`);
    say("a machine behind the local build is marked, and Update brings it up");
    await idle();
    await clickInRow("Rollback");
    await jobDone(`rollback ${service} on ${machine}`);
    await page.until("the badge to come back", `(${row}).querySelector(".badge.behind")`);
    say("Rollback goes back to the build before");
    await idle();
  }

  await clickInRow("Stop");
  await jobDone(`stop ${service} on ${machine}`);
  await page.until("the row to say stopped", `${status} === "stopped"`);
  say("Stop stops the service, as a job");
  await idle();
  await clickInRow("Start");
  await jobDone(`start ${service} on ${machine}`);
  await page.until("the row to say running", `${status} === "running"`);
  say("Start starts it again");
  await idle();

  // The fleet file, from the page: a machine added shows up (unreached,
  // since nothing answers there) and is removed again.
  await page.run(`const f = document.getElementById("machineForm"); f.name.value = "click-test"; f.url.value = "http://127.0.0.1:1"; f.token_env.value = "NO_SUCH_TOKEN"; f.requestSubmit();`);
  await page.until("the new machine in the table", `[...document.querySelectorAll("#machines tbody tr")].some(r => r.children[0].textContent === "click-test")`, 30);
  await page.until("it to be marked unreached", `[...document.querySelectorAll("#machines tbody tr")].some(r => r.children[0].textContent === "click-test" && r.textContent.includes("unreached"))`, 30);
  await page.run(`const r = [...document.querySelectorAll("#machines tbody tr")].find(r => r.children[0].textContent === "click-test"); [...r.querySelectorAll("button")].find(b => b.textContent === "Remove").click();`);
  await page.until("it to be gone from the table", `![...document.querySelectorAll("#machines tbody tr")].some(r => r.children[0].textContent === "click-test")`, 30);
  say("a machine is added to the fleet file from the page, and removed again");

  await page.click("#services", "Deploy");
  await page.until("the deploy job's lines", `[...document.querySelectorAll("#jobs .job")].some(j => j.querySelector("h4").textContent.includes("deploy ${service}") && j.querySelector("pre")?.textContent.includes("running"))`, 90);
  await page.until("the deploy to finish", `[...document.querySelectorAll("#jobs .job")].find(j => j.querySelector("h4").textContent.trim().includes("deploy ${service}") )?.querySelector(".badge").textContent === "done"`, 90);
  say("Deploy rolls the service out, its progress shown line by line");
  page.close();
}

async function hostConsole() {
  console.log("a host's console: " + args.console);
  const build = resolve(args.build);
  const page = await open(`${args.console}#token=${encodeURIComponent(args["console-token"])}`);
  await page.until("the console", `document.title.startsWith("Console") && document.getElementById("status").textContent.startsWith("live")`);
  await page.until("the host in the machines table", `[...document.querySelectorAll("#machines tbody tr")].some(r => r.children[0].textContent === "host")`);
  if (await page.run(`return location.hash`)) throw new Error("the token was left in the address");
  say("the console signs in from its link, drops the token from the address, and lists the host");

  // A service made from a build on "this computer".
  await page.until("the platform filled in", `document.getElementById("serviceForm").platform.value !== ""`);
  await page.run(`const f = document.getElementById("serviceForm"); f.name.value = "clicked"; f.port.value = "${args.port}";`);
  await page.setFile(`#serviceForm input[name="build"]`, build);
  await page.run(`document.getElementById("serviceForm").requestSubmit();`);
  const card = `[...document.querySelectorAll("#services .card")].find(c => c.querySelector("h3").textContent.startsWith("clicked"))`;
  await page.until("the service's card with its uploaded build", `(${card})?.querySelector("dd[title^='builds/']")?.textContent.includes("MB")`, 60);
  say("a service is made from a build uploaded from the page");

  await page.run(`[...(${card}).querySelectorAll("button")].find(b => b.textContent === "Deploy").click();`);
  await page.until("the deploy to finish", `[...document.querySelectorAll("#jobs .job")].some(j => j.querySelector("h4").textContent.includes("deploy clicked") && j.querySelector(".badge").textContent === "done")`, 90);
  const row = `[...document.querySelectorAll("#machines tbody tr")].find(r => r.children[2].textContent === "clicked")`;
  await page.until("the service running", `(${row})?.children[4].textContent === "running"`, 30);
  say("Deploy sends it to the host, and it runs");

  await page.putUpFile(build);
  // What the upload was answered, seen as the page sees it.
  await page.run(`window.__builds = []; const f = window.fetch; window.fetch = async (...a) => { const r = await f(...a);
    if (String(a[0]).includes("api/build/")) window.__builds.push(r.status); return r; };`);
  await page.run(`[...(${card}).querySelectorAll("button")].find(b => b.textContent === "Upload build").click();`);
  await page.until("the upload to be answered", `window.__builds.length > 0`, 60);
  const answered = await page.run(`return window.__builds[0]`);
  if (answered !== 200) throw new Error("the upload was answered " + answered);
  const alerted = await page.run(`return window.__alert || ""`);
  if (alerted) throw new Error("the upload alerted: " + alerted);
  say("Upload build on a service's card sends a build to the host");

  // Redrawing the charts must not move the page: it used to empty them and
  // measure, which on a phone threw the reader back to the top.
  await page.until("charts drawn", `document.querySelectorAll("#machineOverview .chart svg").length > 0`, 30);
  const moved = await page.run(`
    window.scrollTo(0, document.documentElement.scrollHeight);
    const before = window.scrollY;
    renderOverview(true);
    return [before, window.scrollY];`);
  if (moved[0] < 100 || moved[0] !== moved[1]) throw new Error(`the page moved when the charts were drawn: ${moved[0]} -> ${moved[1]}`);
  await page.run(`window.scrollTo(0, 0);`);
  say("drawing the charts again leaves the page where the reader had it");

  // Dashboard opens the server's own dashboard inside the console - in a
  // tab of its own: headless Chrome's intercepted file dialog leaves the
  // tab it opened in unable to do the next such thing.
  page.close();
  const dashPage = await open(`${args.console}#token=${encodeURIComponent(args["console-token"])}`);
  await dashPage.until("the service's Dashboard button", `[...((${row})?.querySelectorAll("button") || [])].some(b => b.textContent === "Dashboard")`, 30);
  await dashPage.run(`[...(${row}).querySelectorAll("button")].find(b => b.textContent === "Dashboard").click();`);
  const inside = `document.getElementById("dashFrame").contentDocument`;
  await dashPage.until("the server's dashboard in the console", `!document.getElementById("dashFrame").hidden && ${inside}?.querySelectorAll("#tiles .tile").length >= 8 && !${inside}.getElementById("status").classList.contains("bad")`, 30);
  if (await dashPage.run(`return document.getElementById("dashPick").value`) !== "host/clicked") throw new Error("the picker does not name the server shown");
  say("Dashboard opens the server's own dashboard inside the console, signed in by the console's token");
  // Its account controls answer through the console too (this server knows
  // players by token alone, so it says it keeps no accounts).
  await dashPage.run(`const d = ${inside}; d.getElementById("account").value = "nobody"; d.defaultView.confirm = () => true; d.getElementById("deleteBtn").click();`);
  await dashPage.until("the account answer", `/no such account|keeps no accounts/.test(${inside}.getElementById("accountOut").textContent)`, 20);
  say("an account action on the server's dashboard reaches the server through the console");

  dashPage.close();
}

try {
  if (args.admin) await adminDashboard();
  if (args.liveops) await liveopsPage();
  if (args.fleet) await fleetInterface();
  if (args.console) await hostConsole();
  console.log(`all ${did.length} steps passed`);
  child.kill();
  process.exit(0);
} catch (error) {
  console.error(`FAILED after "${step}": ${error.message}`);
  child.kill();
  process.exit(1);
}
