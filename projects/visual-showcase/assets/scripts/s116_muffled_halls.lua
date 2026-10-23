-- 116 Muffled Halls: three loops in three rooms, each a spatial
-- `audio_source` with `occlusion` and `muffle`, and the knight the
-- `audio_listener` with a `room_reverb`. The engine counts the walls between
-- them and measures the room; this script only walks the knight, swings the
-- doors and says in words what the engine is doing.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/116-muffled-halls.scene"
local FLOOR, DOOR_H = -300.0, 170.0
local WALLS = { -250.0, 150.0 }
local ROOM_NAMES = { "the courtyard", "the guard room", "the great hall" }
local SOURCES = {
    { name = "Fountain", room = 0 },
    { name = "Hearth", room = 1 },
    { name = "Lute", room = 2 }
}

local knight, lamp, status
local doors, door_open, door_lift = {}, { false, false }, { 0, 0 }
local rings = {}
local captions = true
local t = 0
local dir = 1
local forced = { 0, 0 }
local hurry = 1

local function room_of(x)
    if x < WALLS[1] then return 1 elseif x < WALLS[2] then return 2 end
    return 3
end

-- Closed doors between two rooms - the walls the sound comes through.
local function walls_between(a, b)
    local n = 0
    for w = math.min(a, b), math.max(a, b) - 1 do
        if not door_open[w] then n = n + 1 end
    end
    return n
end

function init(entity)
    knight = Entity.find_by_name("Knight")
    lamp = Entity.find_by_name("KnightLamp")
    status = Entity.find_by_name("Status")
    for i = 1, 2 do doors[i] = Entity.find_by_name("Door" .. (i - 1)) end
    for _, s in ipairs(SOURCES) do
        rings[s.name] = {}
        for k = 1, 3 do rings[s.name][k] = Entity.find_by_name(s.name .. "Ring" .. k) end
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("Space") then hurry = hurry == 1 and 2.5 or 1 end
    -- The knight walks the castle end to end, and back.
    local x, y = Transform.get_position(knight)
    x = x + dir * 95 * hurry * dt
    if x > 680 then dir = -1 elseif x < -680 then dir = 1 end
    Transform.set_position(knight, x, y)
    Transform.set_scale(knight, dir, 1)
    Transform.set_position(lamp, x, y + 60)
    -- A door opens as the knight comes to it and shuts behind, unless D
    -- has propped it open for a while.
    if Input.is_key_just_pressed("D") then forced = { 8, 8 } end
    for i = 1, 2 do
        forced[i] = math.max(0, forced[i] - dt)
        local near = math.abs(x - WALLS[i]) < 140
        door_open[i] = near or forced[i] > 0
        local want = door_open[i] and 1 or 0
        door_lift[i] = door_lift[i] + (want - door_lift[i]) * math.min(1, dt * 6)
        -- Lifted into the wall above: out of the way of the line between
        -- the knight and the sound.
        Transform.set_position(doors[i], WALLS[i], FLOOR + DOOR_H / 2 + door_lift[i] * DOOR_H)
    end
    -- Rings drift out from each sound, fainter through more walls.
    local here = room_of(x)
    local lines = {}
    for _, s in ipairs(SOURCES) do
        local walls = walls_between(here, s.room + 1)
        local a = 0.35 / (1 + walls * 1.5)
        for k = 1, 3 do
            local p = (t * 0.45 + k / 3) % 1
            local k_size = (60 + p * 420) / 100
            Transform.set_scale(rings[s.name][k], k_size, k_size)
            local c = { Sprite.get_color(rings[s.name][k]) }
            Sprite.set_color(rings[s.name][k], c[1], c[2], c[3], a * (1 - p))
        end
        local how = walls == 0 and "clear" or (walls == 1 and "through a wall, muffled" or "through two walls, faint")
        lines[#lines + 1] = s.name:lower() .. " " .. how
    end
    UI.set_text(status, "in " .. ROOM_NAMES[here] .. ":  " .. table.concat(lines, "   "))
end
