-- Scene 53 - Camera FX & Time Scale
--
--   Camera.shake(amplitude, duration, frequency)
--   Camera.flash(duration, r, g, b, a)
--   Time.set_scale(scale)
--
-- set_time_scale is the one with teeth. It multiplies the dt handed to the
-- whole world tick, so 0.0 is a *true* pause: the spinner's AngularVelocity,
-- the drifter's Oscillator, Lifetime countdowns, damage-over-time and
-- animation all stop, because every one of them derives from that dt. Scripts
-- still run at dt = 0, which is how a pause menu keeps reading input.
--
-- It also survives load_scene. A scene you can reach from a paused one has to
-- restore it itself - this script does that in init(), so entering it from a
-- paused scene 20 does not leave you frozen.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/53-camera-fx.scene"

local scale = 1.0
local last = "-"

function init(entity)
    -- Restore, because time scale is global and persists across scenes.
    scale = 1.0
    Time.set_scale(1.0)
    log("[scene_53] ready - S shake, F flash, 1/2/3/0 time scale")
end

function update(entity, dt)
    if Input.is_key_just_pressed("S") then
        Camera.shake(18.0, 0.45, 28.0)
        last = "camera_shake(18, 0.45s, 28Hz)"
    elseif Input.is_key_just_pressed("F") then
        Camera.flash(0.35, 1.0, 0.95, 0.7, 0.8)
        last = "screen_flash(0.35s, warm white)"
    elseif Input.is_key_just_pressed("1") then
        scale = 1.0; Time.set_scale(scale); last = "set_time_scale(1.0)"
    elseif Input.is_key_just_pressed("2") then
        scale = 0.5; Time.set_scale(scale); last = "set_time_scale(0.5)"
    elseif Input.is_key_just_pressed("3") then
        scale = 0.1; Time.set_scale(scale); last = "set_time_scale(0.1)"
    elseif Input.is_key_just_pressed("0") then
        scale = 0.0; Time.set_scale(scale); last = "set_time_scale(0.0) - true pause"
    end

    local sl = Entity.find_by_name("ScaleLabel")
    if sl then
        Text.set(sl, string.format("time scale: %.2f%s", scale,
            scale == 0.0 and "   (paused - scripts still run)" or ""))
    end
    local r = Entity.find_by_name("Readout")
    if r then
        Text.set(r, string.format("last: %s   |   get_current_scene_name() = %s",
            last, tostring(Scene.get_name() or "nil")))
    end

    -- Deliberately last: nav still works at dt = 0, which is the point.
    nav.tick(SCENE_PATH)
end
