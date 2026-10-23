-- 09 Ten Thousand Lights: spawns the lanterns once and then leaves them to
-- the engine - each one's `velocity` carries it up and `wrap_bounds` brings
-- it back at the horizon - so the frame rate is the engine's, not the
-- script bridge's. The status line reads it.

script = { tick_rate = 30 }

local nav = require("nav")
local SCENE = "scenes/09-ten-thousand-lights.scene"

-- tools/gen_scenes.py: TTL_LAYERS counts, TTL_VARIANTS, TTL_WRAP.
local LAYERS = { { "Far", 7600 }, { "Mid", 1900 }, { "Near", 200 }, { "NearLit", 300 } }
local VARIANTS = 4
local X_MIN, X_MAX = -700.0, 700.0
local Y_MIN, Y_MAX = -140.0, 400.0

local captions = true
local post_on = nav.post_all_on(SCENE)
local lights_on = true
local lit = {}
local status
local fps_avg = 0.0
local refresh_in = 0.0

local function refresh_status()
    if not status then return end
    local fps = math.floor(fps_avg + 0.5)
    local count = Debug.get_entity_count and Debug.get_entity_count() or 0
    Text.set(status, fps .. " fps   " .. math.floor(count + 0.5) .. " entities   lights "
        .. (lights_on and "on" or "off") .. "   " .. nav.post_status(post_on))
end

function init(entity)
    status = Entity.find_by_name("Status")
    for _, layer in ipairs(LAYERS) do
        local name, count = layer[1], layer[2]
        for i = 1, count do
            local e = Entity.spawn_prefab("Ln" .. name .. math.random(1, VARIANTS))
            Transform.set_position(e, X_MIN + math.random() * (X_MAX - X_MIN), Y_MIN + math.random() * (Y_MAX - Y_MIN))
            if name == "NearLit" then lit[#lit + 1] = e end
        end
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    if Input.is_key_just_pressed("L") then
        lights_on = not lights_on
        for _, e in ipairs(lit) do Lighting.set_intensity(e, lights_on and 0.9 or 0.0) end
        refresh_status()
    end
    -- A smoothed frame rate, written twice a second.
    local fps = Debug.get_fps and Debug.get_fps() or 0
    fps_avg = fps_avg == 0 and fps or fps_avg + (fps - fps_avg) * 0.1
    refresh_in = refresh_in - dt
    if refresh_in <= 0 then
        refresh_in = 0.5
        refresh_status()
    end
end
