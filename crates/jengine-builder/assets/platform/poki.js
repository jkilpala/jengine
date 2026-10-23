// Poki - SDK https://game-cdn.poki.com/scripts/v2/poki-sdk.js
// (https://developers.poki.com/guide/sdk-html5).
//
// init first, and the game goes on whether it resolves or rejects. The
// loading-finished signal times the load; gameplayStart / gameplayStop
// bracket play and must not be sent during an ad. commercialBreak and
// rewardedBreak take a callback for when the ad really starts (it may not
// come); the rewarded one resolves true when the reward is earned. Poki has
// no loading bar of its own to feed and no scores.

  function poki() { return window.PokiSDK || null; }

  var portal = {
    name: 'poki',
    init: function () {
      var s = poki();
      if (!s) return;
      return s.init().catch(noop);
    },
    loadingFinished: function () {
      var s = poki();
      if (s) s.gameLoadingFinished();
    },
    gameReady: noop,
    gameplayStart: function () {
      var s = poki();
      if (s) s.gameplayStart();
    },
    gameplayStop: function () {
      var s = poki();
      if (s) s.gameplayStop();
    },
    happyTime: noop,
    submitScore: noop,
    showAd: function (kind, started) {
      var s = poki();
      if (!s) return Promise.resolve({ shown: false, rewarded: false });
      var began = false;
      var onStart = function () { began = true; started(); };
      if (kind === 'rewarded') {
        return s.rewardedBreak(onStart).then(function (success) {
          return { shown: began || !!success, rewarded: !!success };
        });
      }
      // Not every commercialBreak shows an ad; Poki decides.
      return s.commercialBreak(onStart).then(function () {
        return { shown: began, rewarded: false };
      });
    }
  };
