// The part of every portal adapter that is the same (docs/web-platforms.md).
//
// `jengine web-build --platform <id>` writes jengine-platform.js as
//   (function () { var CONFIG = {...}; <assets/platform/<id>.js> <this file> })();
// and loads it, after the portal's SDK, before the module script that starts
// the wasm. The portal file defines `portal`, the few calls that differ; this
// file is everything else:
//
// - `window.__jengine_platform`, what the engine sees. The script preamble's
//   `Platform.*` functions call its methods, and the web runtime reads its
//   `paused` / `muted` flags every frame (`jengine-web/src/platform.rs`).
// - The loading bar, for a portal that draws one (`portal.loadingProgress`):
//   the wasm download byte by byte, then each file the engine fetches before
//   its first frame.
// - `portal.loadingFinished()` on the engine's `jengine-ready` (first frame
//   drawn), and the game's readiness after it, by itself or from a script.
// - Ads: the game held - paused and muted - while one plays, gameplay
//   stopped around it, and the answer sent to scripts as the
//   `platform_ad_finished` event.
//
// Plain ES5: it runs before anything else on the page, in whatever browser a
// portal's players have.

  function noop() {}

  // Anything a portal call throws stays here: a portal is a guest in the
  // game's page, and its SDK may be missing (an ad blocker, a local test).
  function safely(fn) {
    try { return fn(); } catch (e) { return undefined; }
  }

  // An event to every script listening (`Event.on(name, entity)`), through
  // the command queue the script preamble drains each frame.
  function emit(name, data) {
    var play = window.__jengine_play;
    if (!play || !play.commands) return;
    play.commands.push({ t: 'evt', n: name, e: 0, d: JSON.stringify(data || null) });
  }

  // Pause and mute are each held for reasons (an ad, the portal), so one
  // reason ending does not undo another.
  var pauses = {}, mutes = {};
  function anyOf(set) {
    for (var k in set) if (set[k]) return true;
    return false;
  }
  function hold(set, reason, on) {
    set[reason] = !!on;
    api.paused = anyOf(pauses);
    api.muted = anyOf(mutes);
  }

  // What `portal.init` gets: how the portal reaches the game.
  var host = {
    config: CONFIG,
    // The portal paused or resumed the game (YouTube's onPause / onResume).
    // Scripts hear `platform_pause` first: the engine runs the frame the
    // pause arrives on, so a game can save before it stops.
    pause: function (on) {
      on = !!on;
      if (!!pauses.portal === on) return;
      emit(on ? 'platform_pause' : 'platform_resume', { reason: 'portal' });
      hold(pauses, 'portal', on);
    },
    // The portal's own audio setting (YouTube, CrazyGames' muteAudio).
    mute: function (on) { hold(mutes, 'portal', on); }
  };

  // Calls wait for `portal.init` (an SDK that must initialise first), and
  // the game's readiness waits for the first frame (YouTube refuses
  // gameReady before firstFrameReady).
  var initDone = false, initQueue = [];
  function afterInit(fn) {
    if (initDone) safely(fn); else initQueue.push(fn);
  }
  var loaded = false, loadedQueue = [];
  function afterLoaded(fn) {
    if (loaded) fn(); else loadedQueue.push(fn);
  }

  var playing = false, adRunning = false, gameReadySent = false;

  function setPlaying(on) {
    on = !!on;
    if (playing === on) return;
    playing = on;
    // Never during an ad: the portal counts it as play.
    if (adRunning) return;
    afterInit(function () { if (on) portal.gameplayStart(); else portal.gameplayStop(); });
  }

  function sendGameReady() {
    if (gameReadySent) return;
    gameReadySent = true;
    afterLoaded(function () { afterInit(function () { portal.gameReady(); }); });
  }

  var api = {
    name: portal.name,
    paused: false,
    muted: false,
    // `fromScript`: `Platform.game_ready()`, which counts only when the
    // project said the game tells (`web_game_ready: "script"`); otherwise
    // the first frame already did.
    gameReady: function (fromScript) {
      if (fromScript && CONFIG.gameReady !== 'script') return;
      sendGameReady();
    },
    gameplayStart: function () { setPlaying(true); },
    gameplayStop: function () { setPlaying(false); },
    happyTime: function () { afterInit(function () { portal.happyTime(); }); },
    submitScore: function (value, board) {
      afterInit(function () { portal.submitScore(value, board); });
    },
    showAd: function (kind, reward) {
      var answer = function (shown, rewarded) {
        emit('platform_ad_finished', { kind: kind, shown: !!shown, rewarded: !!rewarded });
      };
      if (adRunning) { answer(false, false); return; }
      adRunning = true;
      var wasPlaying = playing;
      afterInit(function () {
        if (wasPlaying) portal.gameplayStop();
        var held = false;
        var started = function () {
          if (held) return;
          held = true;
          hold(pauses, 'ad', true);
          hold(mutes, 'ad', true);
        };
        var finish = function (result) {
          hold(pauses, 'ad', false);
          hold(mutes, 'ad', false);
          adRunning = false;
          // The game's own gameplay calls during the ad were held back.
          if (playing) portal.gameplayStart();
          answer(result && result.shown, result && result.rewarded);
        };
        var pending = safely(function () { return portal.showAd(kind, started, reward || ''); });
        if (!pending || typeof pending.then !== 'function') { finish(null); return; }
        pending.then(finish, function () { finish(null); });
      });
    }
  };
  window.__jengine_platform = api;

  // ---- Loading -----------------------------------------------------------

  var realFetch = window.fetch;
  // -1 so the first report, 0, goes out: it is what tells a portal to keep
  // its loading screen up until the first frame.
  var shown = -1, files = 0;
  function progress(p) {
    if (p - shown < 0.01 && p < 1) return;
    shown = p;
    safely(function () { portal.loadingProgress(p); });
  }
  if (portal.loadingProgress && realFetch) {
    progress(0);
    // The wasm by its bytes (its size is known at build time; the server's
    // Content-Length is the compressed one), then every other file the
    // engine fetches before its first frame, each closing part of the gap.
    window.fetch = function (input) {
      var url = typeof input === 'string' ? input : (input && input.url) || String(input);
      var pending = realFetch.apply(window, arguments);
      if (url.indexOf(CONFIG.wasm) === -1) {
        return pending.then(function (resp) {
          files++;
          progress(CONFIG.wasmShare + (0.99 - CONFIG.wasmShare) * (1 - Math.pow(0.85, files)));
          return resp;
        });
      }
      return pending.then(function (resp) {
        if (!resp.ok || !resp.body || typeof ReadableStream === 'undefined') return resp;
        var reader = resp.body.getReader(), got = 0;
        var counted = new ReadableStream({
          pull: function (ctl) {
            return reader.read().then(function (r) {
              if (r.done) { ctl.close(); return; }
              got += r.value.byteLength;
              progress(CONFIG.wasmShare * Math.min(1, got / CONFIG.wasmBytes));
              ctl.enqueue(r.value);
            });
          },
          cancel: function (why) { return reader.cancel(why); }
        });
        // Same headers, so instantiateStreaming still sees application/wasm.
        return new Response(counted, { status: resp.status, statusText: resp.statusText, headers: resp.headers });
      });
    };
  }

  function engineReady() {
    if (loaded) return;
    loaded = true;
    if (window.fetch !== realFetch) window.fetch = realFetch;
    progress(1);
    afterInit(function () { portal.loadingFinished(); });
    var queued = loadedQueue;
    loadedQueue = [];
    for (var i = 0; i < queued.length; i++) queued[i]();
    if (CONFIG.gameReady !== 'script') sendGameReady();
  }
  if (window.__jengine_ready) engineReady();
  else window.addEventListener('jengine-ready', engineReady);

  // ---- Start the portal ----------------------------------------------------

  // Once the page has a body: this file runs in <head>, and an SDK may build
  // its own elements in `init` (CrazyGames does, and threw on a null body).
  // An init that never settles must not hold every call back for good, so
  // after INIT_LIMIT_MS the calls go out anyway - each adapter checks its SDK.
  var INIT_LIMIT_MS = 5000;
  var settle = function () {
    if (initDone) return;
    initDone = true;
    var queued = initQueue;
    initQueue = [];
    for (var i = 0; i < queued.length; i++) safely(queued[i]);
  };
  function startPortal() {
    setTimeout(settle, INIT_LIMIT_MS);
    var started = safely(function () { return portal.init(host); });
    if (started && typeof started.then === 'function') started.then(settle, settle);
    else settle();
  }
  if (document.body) startPortal();
  else document.addEventListener('DOMContentLoaded', startPortal);
