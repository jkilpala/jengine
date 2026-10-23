// Adapts the Colyseus browser SDK to the jengine-net JS transport contract.
//
// Copy this into your project (say `assets/colyseus-adapter.js`) and list it
// after the SDK in `project.jengine`:
//
//   "settings": {
//     "web_scripts": [
//       "https://unpkg.com/@colyseus/sdk@^0.17.0/dist/colyseus.js",
//       "colyseus-adapter.js"
//     ]
//   }
//
// Order matters: the builder emits these as classic scripts in the order
// given, and this file needs `Colyseus` to already exist.
//
// Then connect with a `js:` URL, which routes through this adapter instead of
// opening a raw WebSocket:
//
//   set_custom(net, "Net", "url", "js:my_room")
//
// The engine never learns what Colyseus is; this file is the whole of the
// coupling, and it is yours to edit.

(function () {
  // Where the server lives. Change to your deployment; Colyseus' default dev
  // port is 2567.
  const ENDPOINT = "ws://localhost:2567";

  // The contract is poll-based because the engine reads it once per frame:
  // events pile up here between frames and are drained whole.
  const connections = new Map();
  let nextId = 1;

  function makeState() {
    return { events: [], room: null, closed: false };
  }

  globalThis.jengineNet = {
    // `url` is whatever followed `js:` — for Colyseus that is a room name,
    // not a URL. A library's addressing is its own business.
    connect(url) {
      const id = nextId++;
      const state = makeState();
      connections.set(id, state);

      const client = new Colyseus.Client(ENDPOINT);
      client
        .joinOrCreate(url)
        .then((room) => {
          if (state.closed) {
            // Disconnected while the join was still in flight.
            room.leave();
            return;
          }
          state.room = room;
          state.events.push({ type: "open" });

          // Colyseus dispatches typed messages; "*" catches every type so the
          // game can route them itself. The engine's channel carries text, so
          // the type is folded into the payload rather than lost.
          room.onMessage("*", (type, message) => {
            state.events.push({
              type: "message",
              data: JSON.stringify({ type: String(type), message }),
            });
          });

          // State patches arrive separately from messages. Sending them
          // through as a distinct type keeps them distinguishable.
          if (room.state && typeof room.onStateChange === "function") {
            room.onStateChange((s) => {
              state.events.push({
                type: "message",
                data: JSON.stringify({ type: "__state", message: s }),
              });
            });
          }

          room.onLeave((code) => {
            state.events.push({ type: "close", code: code || 1000, reason: "left" });
          });
          room.onError((code, message) => {
            state.events.push({ type: "error", message: `${code}: ${message}` });
          });
        })
        .catch((err) => {
          state.events.push({ type: "error", message: String(err && err.message || err) });
        });

      return id;
    },

    // The engine sends text. Everything here is JSON `{ type, payload }`,
    // because Colyseus wants a message type and a raw string has none.
    send(id, text) {
      const state = connections.get(id);
      if (!state || !state.room) return;
      let type = "message";
      let payload = text;
      try {
        const parsed = JSON.parse(text);
        if (parsed && typeof parsed.type === "string") {
          type = parsed.type;
          payload = parsed.payload !== undefined ? parsed.payload : parsed;
        }
      } catch (_) {
        // Not JSON. Send it as-is under the default type rather than
        // refusing — a plain string is a legitimate thing to send.
      }
      state.room.send(type, payload);
    },

    // Drain, never peek: returning the same events twice would deliver every
    // message twice, and the engine's transport contract says drain.
    poll(id) {
      const state = connections.get(id);
      if (!state) return [];
      const out = state.events;
      state.events = [];
      return out;
    },

    close(id) {
      const state = connections.get(id);
      if (!state) return;
      state.closed = true;
      if (state.room) state.room.leave();
      connections.delete(id);
    },
  };
})();
