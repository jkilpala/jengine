-- Scene 109: HDR, tonemapping and the bloom mip chain.
--
-- The dots are coloured 0.5 to 16 times white (a sprite's colour is a float,
-- and so is the scene target). With HDR off, everything past 1.0 clips: the
-- four brightest dots are one white and bloom the same. With HDR on, the
-- output pass tonemaps them, so each is brighter than the last and blooms
-- wider. The lamp (intensity 3) lights the floor past its colour only with
-- HDR on; off, light stops at 1.0 as it always did.
--
-- The white panel bottom-right is entity UI. It is drawn after the post
-- chain, so it must never bloom, whatever the settings.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/109-hdr.scene"

local NAMES = { "none", "reinhard", "aces", "agx" }
local hdr, mode, exposure, bloom, scatter = true, 3, 1.0, true, 0.7
local status

local function refresh()
    if not status then return end
    Text.set(status, string.format("hdr %s   tonemap %s   exposure %.2f   bloom %s   scatter %.2f",
        hdr and "ON" or "OFF", NAMES[mode], exposure, bloom and "ON" or "OFF", scatter))
end

function init(entity)
    status = Entity.find_by_name("Status")
    refresh()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local changed = false
    if Input.is_key_just_pressed("T") then
        hdr = not hdr
        PostProcess.set_enabled("hdr", hdr)
        changed = true
    end
    for i = 1, 4 do
        if Input.is_key_just_pressed(tostring(i)) then
            mode = i
            PostProcess.set_float("tonemap", "mode", i - 1)
            changed = true
        end
    end
    if Input.is_key_just_pressed("Q") then exposure = math.max(0.1, exposure / 1.25); changed = true end
    if Input.is_key_just_pressed("W") then exposure = math.min(8.0, exposure * 1.25); changed = true end
    if Input.is_key_just_pressed("A") then scatter = math.max(0.0, scatter - 0.1); changed = true end
    if Input.is_key_just_pressed("S") then scatter = math.min(1.0, scatter + 0.1); changed = true end
    if Input.is_key_just_pressed("B") then
        bloom = not bloom
        PostProcess.set_enabled("bloom", bloom)
        changed = true
    end
    if changed then
        PostProcess.set_float("tonemap", "exposure", exposure)
        PostProcess.set_float("bloom", "scatter", scatter)
        refresh()
    end
end
