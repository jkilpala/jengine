// CrazyGames - SDK v3, https://sdk.crazygames.com/crazygames-sdk-v3.js
// (https://docs.crazygames.com/sdk/intro/, /sdk/game/, /sdk/html5-v3/video-ads).
//
// init is awaited before any other call, and calls are made only where
// `environment` is "crazygames" or "local" - elsewhere it is "disabled" and
// every call throws. loadingStart / loadingStop time the load; gameplayStart /
// gameplayStop bracket play; happytime marks a big moment. requestAd takes
// adStarted / adFinished / adError (an unfilled ad is an adError, and must not
// reward). The game must honour the `muteAudio` setting.

  var cgEnabled = false;
  function cg() { return cgEnabled ? window.CrazyGames.SDK : null; }

  var portal = {
    name: 'crazygames',
    init: function (host) {
      var sdk = window.CrazyGames && window.CrazyGames.SDK;
      if (!sdk) return;
      return sdk.init().then(function () {
        cgEnabled = sdk.environment === 'crazygames' || sdk.environment === 'local';
        if (!cgEnabled) return;
        sdk.game.loadingStart();
        var settings = sdk.game.settings || {};
        host.mute(!!settings.muteAudio);
        sdk.game.addSettingsChangeListener(function (s) { host.mute(!!(s && s.muteAudio)); });
      }).catch(noop);
    },
    loadingFinished: function () {
      var s = cg();
      if (s) s.game.loadingStop();
    },
    gameReady: noop,
    gameplayStart: function () {
      var s = cg();
      if (s) s.game.gameplayStart();
    },
    gameplayStop: function () {
      var s = cg();
      if (s) s.game.gameplayStop();
    },
    happyTime: function () {
      var s = cg();
      if (s) s.game.happytime();
    },
    submitScore: noop,
    showAd: function (kind, started) {
      var s = cg();
      if (!s) return Promise.resolve({ shown: false, rewarded: false });
      return new Promise(function (resolve) {
        s.ad.requestAd(kind === 'rewarded' ? 'rewarded' : 'midgame', {
          adStarted: started,
          adFinished: function () { resolve({ shown: true, rewarded: kind === 'rewarded' }); },
          adError: function () { resolve({ shown: false, rewarded: false }); }
        });
      });
    }
  };
