-- Visual Showcase 2's scene list and the navigation every scene shares.
--
-- SCENES is the one list: the hub draws its menu from it and each scene's
-- prev / next walk it. Like the first showcase's, this module holds no
-- state: `require` caches it natively and not in the editor or on the web,
-- so what must outlive a scene goes in a save slot that is never committed.

local M = {}

local MEMORY = "showcase2_nav"

M.HUB = "scenes/hub.scene"

M.SCENES = {
    {
        path = "scenes/01-lantern-square.scene",
        title = "01   Lantern Square",
        blurb = "A market square at dusk: half-timbered houses in perspective, warm windows, "
            .. "lanterns pooling light on the cobbles, a river reflecting the last of the sky. "
            .. "Walk the square; Q / E turn the camera round it.",
    },
    {
        path = "scenes/02-crystal-hollow.scene",
        title = "02   Crystal Hollow",
        blurb = "A cave lit by torches and glowing crystal, wet rock shining under the light, "
            .. "embers rising and a shaft of daylight from a crack far above.",
    },
    {
        path = "scenes/03-forest-shrine.scene",
        title = "03   Forest Shrine",
        blurb = "A shrine in a clearing of autumn oaks: god rays through the canopy, falling "
            .. "leaves, fireflies and stone lanterns, the trees beyond the focus melting away.",
    },
    {
        path = "scenes/04-the-tavern.scene",
        title = "04   The Tavern",
        blurb = "Inside a tavern of plank and brick: a hearth throwing its light across the "
            .. "floor, candles on the tables, regulars at their rounds.",
    },
    {
        path = "scenes/05-snowbound.scene",
        title = "05   Snowbound",
        blurb = "A village under snow at night: cold moonlight, warm windows, snow falling "
            .. "through the lamplight and drifting in the distance.",
    },
    {
        path = "scenes/06-the-battle.scene",
        title = "06   The Battle",
        blurb = "Side-on to a fight in the ruins, the classic battle view: the party facing "
            .. "three monsters, a diorama behind them, spells that light the whole stage. "
            .. "Space casts.",
    },
}

local function index_of(path)
    for i, s in ipairs(M.SCENES) do
        if s.path == path then return i end
    end
    return nil
end

local function go(path)
    Audio.stop_all()
    Scene.load(path)
end

function M.remember_choice(i)
    Save.set(MEMORY, "chosen", i)
end

function M.last_choice()
    local i = Save.get(MEMORY, "chosen")
    if type(i) == "number" then return math.floor(i + 0.5) end
    local last = Save.get(MEMORY, "scene")
    return index_of(last) or 1
end

function M.tick(current_path)
    if Save.get(MEMORY, "scene") ~= current_path then
        Save.set(MEMORY, "scene", current_path)
    end
    if Input.is_action_just_pressed("hub") then
        go(M.HUB)
        return
    end
    local i = index_of(current_path)
    if not i then return end
    local n = #M.SCENES
    if Input.is_action_just_pressed("next_scene") then
        go(M.SCENES[i % n + 1].path)
    elseif Input.is_action_just_pressed("prev_scene") then
        go(M.SCENES[(i - 2) % n + 1].path)
    end
end

M.go = go

-- Tab hides and shows the captions (entity UI named Title, Blurb, Hint, Status).
function M.toggle_captions(visible)
    visible = not visible
    for _, name in ipairs({ "Title", "Blurb", "Hint", "Status" }) do
        local e = Entity.find_by_name(name)
        if e then UI.set_visible(e, visible) end
    end
    return visible
end

return M
