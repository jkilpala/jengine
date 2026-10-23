-- Scene 184: split screen as one field. Each player's camera has a
-- `screen_rect` - the part of the screen it draws onto - and follows its
-- player; Camera.set_screen_rect changes the layout. The players wander on
-- their own until a key moves them.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/184-split-screen.scene"
local SPEED = 420
local p1, p2, c1, c2, status
local t, mode = 0, 1
local x1, x2 = -300, 300
local LAYOUTS = {
    { "side by side", { 0.0, 0.0, 0.498, 1.0 }, { 0.502, 0.0, 0.498, 1.0 } },
    { "top / bottom", { 0.0, 0.0, 1.0, 0.496 }, { 0.0, 0.504, 1.0, 0.496 } },
    { "inset", { 0.0, 0.0, 1.0, 1.0 }, { 0.68, 0.2, 0.28, 0.32 } },
}

function init(entity)
    p1 = Entity.find_by_name("P1")
    p2 = Entity.find_by_name("P2")
    c1 = Entity.find_by_name("CamP1")
    c2 = Entity.find_by_name("CamP2")
    status = Entity.find_by_name("Status")
end

local function layout(i)
    mode = i
    local l = LAYOUTS[i]
    Camera.set_screen_rect(c1, l[2][1], l[2][2], l[2][3], l[2][4])
    Camera.set_screen_rect(c2, l[3][1], l[3][2], l[3][3], l[3][4])
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    for i = 1, 3 do
        if Input.is_key_just_pressed(tostring(i)) then layout(i) end
    end
    local d1, d2 = 0, 0
    if Input.is_key_pressed("A") then d1 = d1 - 1 end
    if Input.is_key_pressed("D") then d1 = d1 + 1 end
    if Input.is_key_pressed("Left") then d2 = d2 - 1 end
    if Input.is_key_pressed("Right") then d2 = d2 + 1 end
    -- Wander apart and back when nobody is playing.
    if d1 == 0 and d2 == 0 then
        x1 = -300 - 900 * math.sin(t * 0.35)
        x2 = 300 + 900 * math.sin(t * 0.35)
    else
        x1 = math.max(-2000, math.min(2000, x1 + d1 * SPEED * dt))
        x2 = math.max(-2000, math.min(2000, x2 + d2 * SPEED * dt))
    end
    Transform.set_position(p1, x1, -140)
    Transform.set_position(p2, x2, -140)
    Transform.set_position(c1, x1, -40)
    Transform.set_position(c2, x2, -40)
    if status then
        UI.set_text(status, LAYOUTS[mode][1] .. "   red at " .. math.floor(x1 / 200 + 0.5)
            .. "   blue at " .. math.floor(x2 / 200 + 0.5))
    end
end
