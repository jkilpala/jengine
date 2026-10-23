// jCabinet (jWorld, FingerCorner) - SDK /api/v1/sdk/jcabinet.js, served by the
// portal itself. Its own guide: <portal>/jcabinet-sdk-guide.md.
//
// The portal draws the loading screen over the game iframe and fills its bar
// from setLoadingProgress; gameLoaded takes it away and times the load. It
// has rewarded video ads (its overlay, over the iframe) and leaderboards, but
// no midgame ads and no gameplay events. Outside the portal every SDK call
// rejects, which `safely` and the catches swallow.

  function jc() { return window.jCabinet || null; }

  var portal = {
    name: 'jcabinet',
    init: function () {},
    loadingProgress: function (p) {
      var s = jc();
      if (s && s.setLoadingProgress) s.setLoadingProgress(p);
    },
    loadingFinished: function () {
      var s = jc();
      if (s) s.gameLoaded().catch(noop);
    },
    gameReady: noop,
    gameplayStart: noop,
    gameplayStop: noop,
    happyTime: noop,
    // A jCabinet score goes to a named leaderboard, made in the dev portal.
    submitScore: function (value, board) {
      var s = jc();
      if (s && board) s.submitScore(board, value).catch(noop);
    },
    showAd: function (kind, started) {
      var s = jc();
      if (!s || kind !== 'rewarded') return Promise.resolve({ shown: false, rewarded: false });
      started();
      return s.showVideoAd().then(function (r) {
        r = r || {};
        return { shown: !!r.rewarded || r.error === 'ad_skipped', rewarded: !!r.rewarded };
      });
    }
  };
