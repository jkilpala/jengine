#!/usr/bin/env node
// Runtime conformance: run one scene natively and in a web build, and diff
// what its script read.
//
// The script-global ratchets check that every function *binds* in every
// runtime. They cannot see a function that binds and answers wrongly - the
// browsers read what their host published, native reads the world, and a
// field one host never publishes answers a default in silence. The only way
// to see that is to run the same script in both and compare the answers.
//
// The scene (Engine Test Lab's 224 by default) logs one
// `[conformance] name = value` line per probe and `[conformance] done N`.
// This runs it in the native runtime and in Chrome against a web build of
// the project, collects both sets of lines, and fails on any difference.
//
//   node tools/conformance.mjs [project] [--scene scenes/x.scene] [--no-build]
//                              [--web-dir <web build>] [--chrome <path>]
//
// Defaults: projects/engine-test-lab, scenes/224-runtime-conformance.scene,
// the web build at target/web-build/<project name>/release/. Without --no-build it
// builds the native runtime (`cargo build -p jengine-runtime`) and the web
// build (`jengine web-build`) first. The native window opens and closes; keep
// the display awake (an idle display throttles frames - see CLAUDE.md).

import { spawn, spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { existsSync, mkdtempSync, readFileSync, statSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, extname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = resolve(fileURLToPath(new URL('..', import.meta.url)));
const args = process.argv.slice(2);
const flag = (name) => {
  const i = args.indexOf(name);
  if (i < 0) return undefined;
  const v = args[i + 1];
  args.splice(i, 2);
  return v;
};
const noBuild = args.includes('--no-build');
if (noBuild) args.splice(args.indexOf('--no-build'), 1);
const scene = flag('--scene') ?? 'scenes/224-runtime-conformance.scene';
const chromePath = flag('--chrome') ?? 'C:/Program Files/Google/Chrome/Application/chrome.exe';
const project = resolve(ROOT, args[0] ?? 'projects/engine-test-lab');
const webDir = resolve(ROOT, flag('--web-dir') ?? join('target/web-build', basename(project), 'release'));
const TIMEOUT_MS = 120_000;

function run(cmd, cmdArgs, opts = {}) {
  console.log(`> ${cmd} ${cmdArgs.join(' ')}`);
  const r = spawnSync(cmd, cmdArgs, { cwd: ROOT, stdio: 'inherit', shell: process.platform === 'win32', ...opts });
  if (r.status !== 0) throw new Error(`${cmd} failed (${r.status})`);
}

/** `[conformance] name = value` lines out of a stream of text, until `done`. */
function collector(label, resolveDone) {
  const lines = new Map();
  let buffer = '';
  return {
    lines,
    feed(text) {
      buffer += text;
      let nl;
      while ((nl = buffer.indexOf('\n')) >= 0) {
        this.line(buffer.slice(0, nl));
        buffer = buffer.slice(nl + 1);
      }
    },
    line(raw) {
      const at = raw.indexOf('[conformance] ');
      if (at < 0) return;
      const body = raw.slice(at + '[conformance] '.length).replace(/\r$/, '');
      if (body.startsWith('done')) {
        console.log(`  ${label}: ${lines.size} probes`);
        resolveDone();
        return;
      }
      const eq = body.indexOf(' = ');
      if (eq > 0) lines.set(body.slice(0, eq), body.slice(eq + 3));
    },
  };
}

async function runNative() {
  const exe = join(ROOT, 'target/debug', process.platform === 'win32' ? 'jengine-runtime.exe' : 'jengine-runtime');
  if (!existsSync(exe)) throw new Error(`${exe} is missing - build it, or drop --no-build`);
  console.log(`native: ${scene}`);
  let done;
  const finished = new Promise((r) => (done = r));
  const c = collector('native', done);
  const child = spawn(exe, ['--scene', join('assets', scene)], {
    cwd: project,
    env: { ...process.env, RUST_LOG: process.env.RUST_LOG ?? 'info' },
  });
  child.stdout.on('data', (d) => c.feed(d.toString()));
  child.stderr.on('data', (d) => c.feed(d.toString()));
  const exited = new Promise((r) => child.on('exit', r));
  const timeout = new Promise((r) => setTimeout(() => r('timeout'), TIMEOUT_MS));
  const how = await Promise.race([finished, exited.then(() => 'exited'), timeout]);
  child.kill();
  if (how === 'timeout') throw new Error('native: no `[conformance] done` before the timeout');
  if (how === 'exited' && c.lines.size === 0) throw new Error('native: the runtime exited before the scene reported');
  return c.lines;
}

const TYPES = {
  '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.wasm': 'application/wasm',
  '.json': 'application/json', '.png': 'image/png', '.wav': 'audio/wav', '.ogg': 'audio/ogg', '.mp3': 'audio/mpeg',
};

/** The web build, served as is - but with `project.jengine` starting at the scene under test. */
function serve(dir) {
  return new Promise((ok) => {
    const server = createServer((req, res) => {
      const path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
      let file = join(dir, path === '/' ? 'index.html' : path);
      if (!file.startsWith(dir) || !existsSync(file) || statSync(file).isDirectory()) {
        res.writeHead(404).end();
        return;
      }
      let body = readFileSync(file);
      if (basename(file) === 'project.jengine') {
        const p = JSON.parse(body.toString('utf8'));
        p.settings = { ...(p.settings ?? {}), entry_scene: scene };
        body = Buffer.from(JSON.stringify(p));
      }
      res.writeHead(200, { 'content-type': TYPES[extname(file)] ?? 'application/octet-stream', 'cache-control': 'no-store' });
      res.end(body);
    });
    server.listen(0, '127.0.0.1', () => ok(server));
  });
}

async function runWeb() {
  if (!existsSync(join(webDir, 'index.html'))) throw new Error(`no web build at ${webDir} - build it, or pass --web-dir`);
  console.log(`web: ${scene} (${webDir})`);
  const server = await serve(webDir);
  const url = `http://127.0.0.1:${server.address().port}/index.html`;
  const port = 9400 + Math.floor(Math.random() * 400);
  const chrome = spawn(chromePath, [
    `--remote-debugging-port=${port}`, `--user-data-dir=${mkdtempSync(join(tmpdir(), 'jconf-'))}`,
    '--no-first-run', '--no-default-browser-check', '--autoplay-policy=no-user-gesture-required',
    '--window-size=1280,800', 'about:blank',
  ], { stdio: 'ignore' });
  try {
    const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
    let targets;
    for (let i = 0; i < 100 && !targets; i++) {
      try { targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json(); } catch { await sleep(200); }
    }
    if (!targets) throw new Error('web: Chrome did not open its debugging port');
    const ws = new WebSocket(targets.find((t) => t.type === 'page').webSocketDebuggerUrl);
    await new Promise((r) => (ws.onopen = r));
    let id = 0;
    const send = (method, params = {}) => ws.send(JSON.stringify({ id: ++id, method, params }));
    let done;
    const finished = new Promise((r) => (done = r));
    const c = collector('web', done);
    const errors = [];
    ws.onmessage = (ev) => {
      const m = JSON.parse(ev.data);
      if (m.method === 'Runtime.consoleAPICalled') {
        const text = m.params.args.map((a) => a.value ?? a.description ?? '').join(' ');
        c.line(text);
      } else if (m.method === 'Runtime.exceptionThrown') {
        errors.push(m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text);
      }
    };
    send('Runtime.enable');
    send('Page.navigate', { url });
    const timeout = new Promise((r) => setTimeout(() => r('timeout'), TIMEOUT_MS));
    if ((await Promise.race([finished, timeout])) === 'timeout') {
      throw new Error(`web: no \`[conformance] done\` before the timeout${errors.length ? ` - ${errors.join('; ')}` : ''}`);
    }
    ws.close();
    return c.lines;
  } finally {
    chrome.kill();
    server.close();
  }
}

if (!noBuild) {
  run('cargo', ['build', '-p', 'jengine-runtime']);
  run('cargo', ['run', '-q', '-p', 'jengine-builder', '--bin', 'jengine', '--', 'web-build', project, '--no-report']);
}
const native = await runNative();
const web = await runWeb();

const names = [...new Set([...native.keys(), ...web.keys()])];
const differ = names.filter((n) => native.get(n) !== web.get(n));
const errors = names.filter((n) => (native.get(n) ?? '').startsWith('ERROR') || (web.get(n) ?? '').startsWith('ERROR'));
console.log('');
for (const n of names) {
  const mark = differ.includes(n) ? 'DIFF' : '    ';
  if (differ.includes(n)) {
    console.log(`${mark} ${n}\n       native: ${native.get(n) ?? '(missing)'}\n       web:    ${web.get(n) ?? '(missing)'}`);
  }
}
console.log(`\n${names.length} probes, ${differ.length} differ, ${errors.length} raised an error`);
process.exit(differ.length || errors.length ? 1 : 0);
