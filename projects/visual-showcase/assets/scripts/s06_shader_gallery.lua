-- 06 Shader Gallery: drives the one effect that needs the game - the
-- dissolve's `progress`, a material parameter (a MaterialParam node in the
-- graph) - and otherwise leaves the graphs to run on their own clock.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/06-shader-gallery.scene"

local CYCLE = 6.0      -- seconds: hold whole, burn away, hold gone, come back
local t = 0.0
local paused = false
local captions = true
local post_on = nav.post_all_on(SCENE)
local dissolve, status

local function refresh_status()
    if status then Text.set(status, (paused and "dissolve paused   " or "") .. nav.post_status(post_on)) end
end

function init(entity)
    dissolve = Entity.find_by_name("Dissolve")
    status = Entity.find_by_name("Status")
    refresh_status()
end

-- 0 while whole, rising to 1 as it burns, 1 while gone, falling back.
local function progress(k)
    if k < 0.2 then return 0.0 end
    if k < 0.5 then return (k - 0.2) / 0.3 end
    if k < 0.6 then return 1.0 end
    return 1.0 - (k - 0.6) / 0.4
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Space") then paused = not paused; refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    if not paused then t = t + dt end
    if dissolve then
        -- The noise runs 0..1 but rarely reaches its ends; stretch a little
        -- past them so the crate is wholly there and wholly gone.
        local p = -0.05 + progress((t % CYCLE) / CYCLE) * 1.1
        Material.set_vec4(dissolve, "progress", p, 0, 0, 0)
    end
end
