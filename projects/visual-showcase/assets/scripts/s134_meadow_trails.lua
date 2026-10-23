-- 134 Meadow Trails: grass that parts round whoever moves through it. The
-- meadow is a `displacement_field`; the fox and the mouse are `displacer`s
-- pressing it outward, and every blade, reed and flower is a `field_bend`
-- leaning away from the push where it stands and springing back after. This
-- script only runs the fox round its figure of eight and keeps the mouse's
-- displacer under the pointer.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/134-meadow-trails.scene"
local fox, hand, status
local captions, running = true, true
local t, last_x = 0, 0
local mx0, my0, still = 0, 0, 0

function init(entity)
    fox = Entity.find_by_name("Fox")
    hand = Entity.find_by_name("Hand")
    status = Entity.find_by_name("Status")
end

local function fox_at(s)
    -- A figure of eight across the meadow, round the pond's side.
    return math.sin(s) * 560, -120 + math.sin(s * 2) * 170
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("F") then running = not running end
    if running then t = t + dt * 0.42 end
    local x, y = fox_at(t)
    Transform.set_position(fox, x, y + (running and math.abs(math.sin(t * 22)) * 6 or 0))
    -- Facing the way it runs.
    if x > last_x + 0.01 then
        Transform.set_scale(fox, 1, 1)
    elseif x < last_x - 0.01 then
        Transform.set_scale(fox, -1, 1)
    end
    last_x = x
    -- The mouse presses the grass where it is (out of the way when it rests).
    local mx, my = Input.get_mouse_world_position()
    if math.abs(mx - mx0) + math.abs(my - my0) > 0.5 then
        still = 0
    else
        still = still + dt
    end
    mx0, my0 = mx, my
    if still < 2 then
        Transform.set_position(hand, mx, my)
    else
        Transform.set_position(hand, 0, -2000)
    end
    UI.set_text(status, running and "the fox runs - the grass parts round it and stands again behind"
        or "the fox rests")
end
