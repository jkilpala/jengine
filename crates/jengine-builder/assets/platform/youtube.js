// YouTube Playables - SDK https://www.youtube.com/game_api/v1, loaded before
// any game code (https://developers.google.com/youtube/gaming/playables/reference/sdk).
//
// firstFrameReady when something is on screen, then gameReady once the game
// can be played (never during a splash or loading screen - certification
// fails it); YouTube keeps its spinner until then. onPause means stop
// everything - updates, drawing, sound, input - until onResume, and save
// first; the engine holds the game and scripts hear `platform_pause`. The
// audio setting must be honoured. sendScore takes an integer best score.
// Ads are experimental there: an interstitial gives no word on whether it
// showed, and a rewarded one needs a reward id per kind of reward.
// Locally the SDK does nothing.

  function yt() { return typeof window.ytgame !== 'undefined' ? window.ytgame : null; }

  var portal = {
    name: 'youtube',
    init: function (host) {
      var y = yt();
      if (!y) return;
      y.system.onPause(function () { host.pause(true); });
      y.system.onResume(function () { host.pause(false); });
      // Only inside YouTube: elsewhere the SDK does nothing, and its audio
      // answer is no reason to play a test build in silence.
      if (!y.IN_PLAYABLES_ENV) return;
      host.mute(y.system.isAudioEnabled() === false);
      y.system.onAudioEnabledChange(function (enabled) { host.mute(enabled === false); });
    },
    loadingFinished: function () {
      var y = yt();
      if (y) y.game.firstFrameReady();
    },
    gameReady: function () {
      var y = yt();
      if (y) y.game.gameReady();
    },
    gameplayStart: noop,
    gameplayStop: noop,
    happyTime: noop,
    submitScore: function (value) {
      var y = yt();
      if (y) y.engagement.sendScore({ value: value }).catch(noop);
    },
    showAd: function (kind, started, reward) {
      var y = yt();
      if (!y || !y.ads) return Promise.resolve({ shown: false, rewarded: false });
      // YouTube pauses the game around its own ad (onPause), so nothing is
      // held here.
      if (kind === 'rewarded') {
        return y.ads.requestRewardedAd(reward || 'reward').then(function (earned) {
          return { shown: true, rewarded: !!earned };
        });
      }
      return y.ads.requestInterstitialAd().then(function () {
        return { shown: true, rewarded: false };
      });
    }
  };
