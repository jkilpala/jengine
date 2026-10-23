-- 136 Fog of War: a campaign map dark but for what our scouts see. Every
-- `sight` clears a `fog_of_war` round it and what it leaves stays explored,
-- dimmed; the enemy's tents, fires and raider are `fog_hidden`, drawn by no
-- pass unless a scout sees them now. The mouse sends the rider; left alone
-- it rides out to the camps one by one.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/136-fog-of-war.scene"
local fog, rider, status
local captions = true
local idle, goal = 3, 1
local tx, ty = nil, nil
local found = {}
local TENTS = 8
local WAYS = { { -250, 40 }, { 140, 240 }, { 470, 200 }, { 560, -60 }, { 600, -230 }, { 100, -260 }, { -150, -280 }, { -420, -60 } }
local SPEED = 110

function init(entity)
    fog = Entity.find_by_name("Fog")
    rider = Entity.find_by_name("Rider")
    status = Entity.find_by_name("Status")
end

local function heading(dx, dy)
    if math.atan2 ~= nil then return math.atan2(dy, dx) end
    return math.atan(dy, dx)
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_mouse_left_pressed() then
        tx, ty = Input.get_mouse_world_position()
        idle = 0
    else
        idle = idle + dt
    end
    if idle > 4 then
        -- Left alone: ride the rounds of the camps.
        local w = WAYS[goal]
        tx, ty = w[1], w[2]
    end
    if tx ~= nil then
        local x, y = Transform.get_position(rider)
        local dx, dy = tx - x, ty - y
        local d = math.sqrt(dx * dx + dy * dy)
        if d > 4 then
            local step = math.min(d, SPEED * dt)
            Transform.set_position(rider, x + dx / d * step, y + dy / d * step)
            Transform.set_rotation(rider, heading(dx, dy))
        elseif idle > 4 then
            goal = goal % #WAYS + 1
        end
    end
    local n = 0
    for i = 1, TENTS do
        local tent = Entity.find_by_name("Tent" .. (i - 1))
        if not found[i] and Entity.get_custom(tent, "Fog", "visible") == true then found[i] = true end
        if found[i] then n = n + 1 end
    end
    local explored = Entity.get_custom(fog, "FogOfWar", "explored") or 0
    UI.set_text(status, "explored " .. math.floor(explored * 100 + 0.5) .. "%    enemy tents found " .. n .. " / " .. TENTS)
end
