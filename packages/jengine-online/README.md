# jengine-online

A game's side of [`jengine-server`](../../crates/jengine-server/README.md):
sign in, join a room, and see what the server says is near you as entities in
the scene.

The server keeps a world and tells each player what is in view. This package
is the other half: it speaks the protocol, keeps the connection alive, and
moves scene entities to where the server's entities are - smoothly, between
the server's ticks.

## Opting in

It needs `jengine-net` beside it (that package moves the bytes; this one
never links it). List both in `project.jengine`:

```json
"packages": [
  { "name": "jengine-net",    "source": { "type": "path", "path": "../../packages/jengine-net" } },
  { "name": "jengine-online", "source": { "type": "path", "path": "../../packages/jengine-online" } }
]
```

and rebuild the project's viewport. Its components need a **v2** scene.

## What you author

One entity is the connection:

```json
"online": { "server": "ws://127.0.0.1:9001", "name": "Ada", "kind": "zone", "room": "meadow" }
```

| Field | Meaning |
|---|---|
| `server` | the server's address |
| `name` | the name to sign in with |
| `token` | who the player is, from an earlier session; empty makes a new player |
| `kind`, `room` | a room to join as soon as signed in; empty `kind` joins nothing |
| `auto_connect` | connect when the scene starts (default `true`) |
| `smoothing` | seconds a position takes to be walked to; `0` is one server tick, measured |
| `retry_secs` | seconds before a dropped connection is tried again (default 3; `0` never) |

And a **pool of slots** - the entities the server's entities are shown with.
A slot is any entity you like (a sprite, children, a script) saying which
`kind` it shows:

```json
{ "name": "Player 1", "components": { "online_slot": { "kind": "player" }, "sprite": { } } }
{ "name": "Coin 1",   "components": { "online_slot": { "kind": "coin" },   "sprite": { } } }
```

When the server says a `player` came into view, a free `player` slot is bound
to it: moved to its position every frame and given a `net_entity` bag with the
server's properties for it. An entity the room turns (`Space::set_rotation`)
turns its slot too, walked between ticks the short way round, with the walked
facing as `rot` in the bag; one it never turns leaves the slot's rotation to
the scripts. `"turn"` on the slot says what turns instead: the name of an
entity under it (a ship's hull, so its name label stays level) or `"none"`.
The facing is written before the scripts run, so a script may still turn its
own avatar after it - a ship's bow answers the helm at once that way. When it leaves view the slot
is freed. An unbound slot is scaled to nothing, children and all; bound again
it gets its authored scale back **once**, and is then the scripts' to scale
(a cell sized by how much it has eaten), and any `trail` on it or under it is
cleared, so a slot handed to another entity draws no streak from where the
last one was. A slot with an empty `kind` takes any kind that has no slot of
its own left.

The pool has a size: an entity with no free slot is not shown, and `overflow`
says how many.

A scene with **two connections** (a split screen, two accounts, two servers)
gives each slot the `session` it shows for - the **name** of its `online`
entity (`"online_slot": { "kind": "player", "session": "Player Two" }`). A
slot naming none shows for the first `online` entity, which is every slot in
a scene with one.

## From a script

```lua
local conn = Entity.find_by_name("Online")

function update(dt)
  local online = Entity.get_custom(conn, "online", "status")
  if online == "joined" and Input.is_mouse_just_pressed() then
    local x, y = Input.get_mouse_world_position()
    Online.send(conn, { go = { x, y } })       -- a table; the room decides what it means
  end
  for _, event in ipairs(Entity.get_custom(conn, "online", "events") or {}) do
    if event.t == "coins" then show_coins(event.coins) end
  end
end
```

| Call | Does |
|---|---|
| `Online.connect(e, server, name, token)` | connect and sign in; empty arguments keep what the component says |
| `Online.join(e, kind, room, options)` | join a room, now or as soon as signed in; empty `room` asks for any |
| `Online.send(e, body)` | a message to the room; one too big for a frame goes in parts the server puts back together |
| `Online.send_latest(e, key, body, per_second)` | the newest `body` under `key`, sent at most `per_second` times a second (default 10) - a helm, an aim, a stroke |
| `Online.fetch(e, id, path, body, method)` | an HTTP request to the game server's own routes (`/api/..`), as the player - the session's token is the bearer; answered by a `fetched` event `{ id, status, ok, body, error }`, `body` decoded if JSON. A `nil` body is a GET, a table a POST |
| `Online.input(e, x, y)` | steer the player's own avatar (each -1..1; `0, 0` stops), **predicted**: it moves at once and is reconciled with the server |
| `Online.leave(e)` | come out of the room |
| `Online.subscribe(e, channel)` / `Online.unsubscribe(e, channel)` | join or leave a channel: chat beside the room, kept from room to room |
| `Online.account(e, request)` | on a server with accounts: `{ change_password = { old = .., new = .. } }` or `{ sign_out_everywhere = true }`. The answer is an `account` event; the package keeps the new token |
| `Online.say(e, channel, body)` | speak in it; everyone in it gets a `said` event (`channel`, `from`, `name`, `body`) |
| `Online.disconnect(e)` | close the connection and stay offline |

The `online` bag on the connection's entity:

| Field | |
|---|---|
| `status` | `offline`, `signing_in`, `signed_in`, `joining`, `joined` |
| `player`, `name` | who the server says we are |
| `token` | **save it** - presenting it next time is what makes it the same player. `Save.set("online", "token", event.token)` on the `welcome` event, and pass it to `Online.connect` next launch; Test Lab scene 221 does |
| `state` | the player's saved state, as of signing in |
| `room` | `{ kind, id }` of the room we are in |
| `you` | the id of our own entity, if the room says (`"you"` on its `joined` frame) |
| `joined` | the `joined` frame, for whatever the room put on it |
| `error` | the last refusal |
| `count`, `overflow` | entities in view, and how many of them have no slot |
| `events` | every frame the server sent this frame except `sync`, as tables |

On each bound slot, the `net_entity` bag: `id`, `kind`, `mine` (it is our own
entity), and whatever the room set on it - `name`, `hp`.

## Steering, predicted

`Online.send(e, { go = { x, y } })` asks the server to walk the avatar, and it
moves when the server moves it: a round trip and a tick later. For direct
control use `Online.input(e, x, y)` instead. The package sends one numbered
input per fixed step and applies it to the player's own slot **at once**,
with the same step function the server runs (`jengine-netcode`, linked by
both ends; the room sends its numbers on `joined`). When the server answers
it starts from there and replays what the server has not seen - which, with
a shared step function, changes nothing. If the server knew better (a wall,
a shove), the difference is eased out rather than snapped.

```lua
local x = (Input.is_key_pressed("D") and 1 or 0) - (Input.is_key_pressed("A") and 1 or 0)
local y = (Input.is_key_pressed("W") and 1 or 0) - (Input.is_key_pressed("S") and 1 or 0)
Online.input(conn, x, y)
```

The player's own slot gets `predicted`, `server_x` and `server_y` in its
`net_entity` bag; the `online` bag gets `predicting`, `unacknowledged` and
the server's `tick`. Everyone else is drawn as before, a tick in the past.
It needs a room that takes `input` messages and says `"netcode"` on `joined`
- the server's zones example and its `world` template do. A room with walls
sends them too (`netcode.walls`, a grid of solid cells) and the prediction is
stopped by the same walls the server stops it with - the zones example builds
them from an engine `Tilemap` its monsters path through.

## A room that changes under the player

Two frames are the package's, not a script's, so a room should not use their
names for frames of its own:

- `{ "t": "netcode", "netcode": { .. } }` - the shape sent on `joined`, sent
  again mid-stay when the walls change (a bridge mended, a fence built). The
  prediction steers by the new walls from the next step.
- `{ "t": "you", "you": id }` - the player's avatar is another entity now (a
  respawn as something else). The new entity is `mine`, and is what is
  predicted.

## A dropped connection

The package reconnects on its own, signs in with the token it was given, and
goes back to the room it was in - with the options it joined that room with,
so a room that reads a code or a seat count from them knows its own player
(a room it was transferred into, which it never asked for, gets none). If
that kind of room holds seats (`grace` on
the server) and it is back in time, nobody saw it go: same avatar, same place.
Otherwise the others see the player leave and return - still the same player,
with what they had. The channels it was in are joined again by themselves.

**Replaced is not dropped.** When the same player signs in somewhere else -
another browser tab (tabs share a save, and so the token), or a second copy
run without `--instances` - the server lets the older connection go with
`{ "t": "error", "why": "signed in somewhere else" }`. The package then
stays off and raises `{ "t": "replaced" }` instead of `dropped`: coming back
would push the other window off, and the two used to trade the player back
and forth, each showing "Connection lost" while it played on. Connect again
to take the player back.

**Signing in again** - `Online.disconnect(e)` then `Online.connect(e, ..)`
in the same frame, to change the name or the server - opens a new socket:
whatever follows a disconnect waits for the next frame, by which time the
old socket is closed. (It used to share the old socket and send a second
`hello` down it, which the server refuses: "already signed in".)

## What it does not do

- **Spawn anything.** It binds slots the scene already has. That is what makes
  it the same in every runtime; the price is a pool with a size.
- **Predict anything but movement.** The player's own avatar is predicted
  when steered with `Online.input`; a shot, a pickup or a spawn is not - it
  happens when the server says. And the movement rules built in are
  top-down (a direction, a speed, a rectangle) and that over a grid of
  square walls.

`projects/engine-test-lab` scene 221 is the worked example. It has been run in
tests (two hosted clients against a real server) and on screen in the native
runtime; **not yet in the editor or a web build**.
