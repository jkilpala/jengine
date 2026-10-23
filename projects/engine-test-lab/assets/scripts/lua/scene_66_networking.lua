-- Scene 66 - Networking (jengine-net package)
--
-- Everything here goes through one custom component. The package has no
-- script globals of its own, deliberately: a package's `register_script_api`
-- is only installed by `ScriptSystem` (native Luau / QuickJS), while the
-- editor viewport and web builds run Lua through Fengari with a hardcoded
-- bridge allowlist. A `Net.send()` global would therefore be nil in the very
-- runtime this engine is developed in. `get_custom` / `set_custom` are core
-- bridge functions that exist everywhere, so this scene behaves identically
-- in all of them.
--
-- The channel, on the entity named "Net":
--
--   url      (script writes) which server to be on; "" means disconnect
--   outbox   (script writes) strings to send; the package sends and clears
--   inbox    (package writes) strings received; the script clears
--   state    (package writes) disconnected/connecting/connected/closed/failed
--   dropped  (package writes) messages lost to a full inbox this session
--   requests  (script writes) HTTP: { id, url, method?, headers?, body? }
--   responses (package writes) { id, status, ok, body, error }
--
-- `url` is declarative rather than a connect() call: a script says which
-- server it wants and the package reconciles. That survives a scene reload
-- running init() again, which a connect-once-and-remember approach does not.
--
-- Nothing connects on load. Every connection here is a keypress, so opening
-- the scene never sends traffic anywhere.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/66-networking.scene"

local LOCAL_URL = "ws://127.0.0.1:2567"
local ECHO_URL = "wss://echo.websocket.org"

local net, state_value, url_value, sent_value, recv_value, http_value
local sent = 0
local received = {}

local function set_url(url)
    if net == nil then return end
    Entity.set_custom(net, "Net", "url", url)
end

local function queue(msg)
    if net == nil then return end
    -- Read-modify-write: the package drains this list, so appending rather
    -- than replacing avoids clobbering anything queued earlier this frame.
    local out = Entity.get_custom(net, "Net", "outbox") or {}
    table.insert(out, msg)
    Entity.set_custom(net, "Net", "outbox", out)
end

-- One HTTP GET. `id` is how the answer is matched back: nothing about the
-- reply says which request it belongs to except this.
local function http_get(id, url)
    if net == nil then return end
    local reqs = Entity.get_custom(net, "Net", "requests") or {}
    table.insert(reqs, {
        id = id,
        url = url,
        method = "GET",
        -- GitHub rejects requests without one, which makes this a useful
        -- demonstration that headers actually arrive.
        headers = { ["User-Agent"] = "jEngine-test-lab" },
    })
    Entity.set_custom(net, "Net", "requests", reqs)
end

function init(entity)
    net = Entity.find_by_name("Net")
    http_value = Entity.find_by_name("HttpValue")
    state_value = Entity.find_by_name("StateValue")
    url_value = Entity.find_by_name("UrlValue")
    sent_value = Entity.find_by_name("SentValue")
    recv_value = Entity.find_by_name("RecvValue")
    log("[scene_66] ready - press 1, 2 or 0; nothing connects on its own")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if net == nil then return end

    if Input.is_key_just_pressed("1") then
        set_url(LOCAL_URL)
        Text.set(url_value, "url: " .. LOCAL_URL)
    elseif Input.is_key_just_pressed("2") then
        set_url(ECHO_URL)
        Text.set(url_value, "url: " .. ECHO_URL)
    elseif Input.is_key_just_pressed("0") then
        set_url("")
        Text.set(url_value, "url: (none)")
    end

    if Input.is_key_just_pressed("3") then
        http_get("zen", "https://api.github.com/zen")
        Text.set(http_value, "requesting...")
    end

    if Input.is_key_just_pressed("Space") then
        sent = sent + 1
        queue(string.format('{"type":"ping","n":%d}', sent))
        Text.set(sent_value, sent .. " messages")
    end

    -- State is republished every frame, so a disconnect shows up even on a
    -- frame with no traffic.
    local state = Entity.get_custom(net, "Net", "state")
    if state ~= nil then
        Text.set(state_value, state)
    end

    -- Drain the inbox. The package appends, so a script that reads every few
    -- frames misses nothing -- but clearing is the script's job, and a script
    -- that never clears will hit the per-frame cap and be told so in the log.
    local inbox = Entity.get_custom(net, "Net", "inbox")
    if inbox ~= nil and #inbox > 0 then
        for _, msg in ipairs(inbox) do
            table.insert(received, msg)
            -- Keep the readout short; this is a display buffer, not a log.
            if #received > 6 then
                table.remove(received, 1)
            end
        end
        Entity.set_custom(net, "Net", "inbox", {})
        Text.set(recv_value, table.concat(received, "\n"))
    end

    -- HTTP answers arrive on their own list. A non-2xx status is still an
    -- answer and carries a body; only `error` means the request never reached
    -- a server at all.
    local responses = Entity.get_custom(net, "Net", "responses")
    if responses ~= nil and #responses > 0 then
        local r = responses[#responses]
        if r.error ~= nil then
            Text.set(http_value, "error: " .. tostring(r.error))
        else
            Text.set(http_value, string.format("%d  %s", r.status, tostring(r.body)))
        end
        Entity.set_custom(net, "Net", "responses", {})
    end
end
