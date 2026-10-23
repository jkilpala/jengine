-- Scene 19 - ScoreKeeper hub.
--
-- Demonstrates persistent state surviving scene transitions:
--   * `persistent_set_number` / `persistent_get_number`
--   * `persistent_set_string` / `persistent_get_string`
--   * `persistent_clear`
--
-- Each sub-scene (19a/b/c) bumps the score and pop_scene's back, so
-- the score visible here grows across visits without ever being
-- stored in a Lua module-level variable (those die when the VM is
-- recycled on every scene transition).

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/19-scorekeeper.scene"

local SCORE_KEY      = "score19"
local PLAYER_KEY     = "player19"
local VISITS_KEY     = "visits19"
local LAST_KEY       = "last19"

local PLAYER_NAMES = { "Alice", "Bob", "Carol", "Dave" }

local score_id, player_id, last_id, visits_id

local prev = {}
local function edge(key)
    local now = Input.is_key_pressed(key)
    local pressed = now and not prev[key]
    prev[key] = now
    return pressed
end

local function refresh()
    if score_id then
        local score = Persistent.get_number(SCORE_KEY) or 0
        Text.set(score_id, string.format("Score: %d", math.floor(score)))
    end
    if player_id then
        local name = Persistent.get_string(PLAYER_KEY) or "(unnamed)"
        Text.set(player_id, "Player: " .. name)
    end
    if last_id then
        local last = Persistent.get_string(LAST_KEY) or "(none)"
        Text.set(last_id, "Last: " .. last)
    end
    if visits_id then
        local visits = Persistent.get_number(VISITS_KEY) or 0
        Text.set(visits_id, string.format("Visits to hub: %d", math.floor(visits)))
    end
end

function init(entity)
    score_id  = Entity.find_by_name("ScoreLabel")
    player_id = Entity.find_by_name("PlayerLabel")
    last_id   = Entity.find_by_name("LastAction")
    visits_id = Entity.find_by_name("Visits")

    -- First-time default for the player name.
    if not Persistent.has(PLAYER_KEY) then
        Persistent.set_string(PLAYER_KEY, PLAYER_NAMES[1])
    end

    -- Count each visit to this hub (init runs every time we land here
    -- whether via load_scene or pop_scene from a sub-scene).
    local v = (Persistent.get_number(VISITS_KEY) or 0) + 1
    Persistent.set_number(VISITS_KEY, v)

    refresh()
end

local function cycle_player()
    local current = Persistent.get_string(PLAYER_KEY) or PLAYER_NAMES[1]
    local idx = 1
    for i, n in ipairs(PLAYER_NAMES) do
        if n == current then idx = i; break end
    end
    idx = (idx % #PLAYER_NAMES) + 1
    Persistent.set_string(PLAYER_KEY, PLAYER_NAMES[idx])
    Persistent.set_string(LAST_KEY, "renamed -> " .. PLAYER_NAMES[idx])
end

function update(entity, dt)
    if not score_id then init(entity) end
    nav.tick(SCENE_PATH)

    if edge("R") then Scene.load(SCENE_PATH) end
    if edge("C") then
        Persistent.clear()
        Persistent.set_string(PLAYER_KEY, PLAYER_NAMES[1])
        Persistent.set_string(LAST_KEY, "cleared persistent store")
    end
    if edge("N") then cycle_player() end
    if edge("1") then Scene.push("scenes/19a-coin.scene") end
    if edge("2") then Scene.push("scenes/19b-gem.scene") end
    if edge("3") then Scene.push("scenes/19c-bonus.scene") end

    refresh()
end
