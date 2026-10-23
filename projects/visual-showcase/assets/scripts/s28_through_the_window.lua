-- 28 Through the Window: carries the portal. The clipping is the engine's
-- (`sprite_mask`, `masked`); this moves the portal and sizes it.
-- Mouse: move the portal (it drifts when left alone).  Up / Down: its size.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/28-through-the-window.scene"

local post_on = nav.post_all_on(SCENE)
local captions = true
local status, portal
local t = 0.0
local size = 1.0
local px, py = -330.0, 40.0
local last_mx, last_my = nil, nil
local quiet = 99.0

local function refresh_status()
    if status then
        Text.set(status, "portal " .. math.floor(size * 100 + 0.5) .. "%   " .. nav.post_status(post_on))
    end
end

function init(entity)
    status = Entity.find_by_name("Status")
    portal = Entity.find_by_name("Portal")
    refresh_status()
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    t = t + dt

    local was = size
    if Input.is_key_pressed("Up") then size = math.min(2.0, size + dt * 0.8) end
    if Input.is_key_pressed("Down") then size = math.max(0.3, size - dt * 0.8) end
    if size ~= was then refresh_status() end

    -- The mouse carries the portal; left alone, it drifts round the wall.
    local mx, my = Input.get_mouse_world_position()
    if last_mx and (math.abs(mx - last_mx) > 0.5 or math.abs(my - last_my) > 0.5) then quiet = 0.0 end
    last_mx, last_my = mx, my
    quiet = quiet + dt
    local tx, ty
    if quiet < 3.0 then
        tx, ty = mx, my
    else
        tx, ty = math.sin(t * 0.23) * 380, 30 + math.sin(t * 0.41) * 120
    end
    local k = 1 - math.exp(-dt * 6)
    px, py = px + (tx - px) * k, py + (ty - py) * k
    if portal then
        Transform.set_position(portal, px, py)
        local breathe = 1 + 0.03 * math.sin(t * 2.2)
        Transform.set_scale(portal, size * breathe, size * breathe)
    end
end
