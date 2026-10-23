-- Scene 49 - Tween API
--
-- Scene 41 covers the Tween *component* (declared in the scene, ticked by the
-- engine). These are the script-side functions that create tweens at runtime
-- and hand back an id you can cancel:
--
--   Tween.position(e, x, y, duration, easing)      -> id
--   Tween.rotation(e, radians, duration, easing)   -> id
--   Tween.scale(e, sx, sy, duration, easing)       -> id
--   Tween.color(e, r, g, b, a, duration, easing)   -> id
--   Tween.cancel(id) / Tween.cancel_all(e)
--
-- `easing` goes through EasingFunction::from_name, which accepts linear,
-- ease_in_quad, ease_out_quad, ease_in_out_quad and the cubic / sine variants.
-- An unknown name logs a warning and falls back to linear - so a typo is a
-- silent slowdown, not an error.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/49-tween-api.scene"

local HOME = { -450.0, -150.0, 150.0, 450.0 }
local HOME_Y = 60.0
local LIFT_Y = 150.0 -- tween 1's other end: clear of the hint text above
local PURPLE = { 0.9, 0.55, 0.95, 1.0 } -- Subject4's authored colour
local YELLOW = { 1.0, 0.95, 0.3, 1.0 }
local ids = {}
local last = "-"
-- Every tween goes back and forth, so every press shows something. Each used
-- to aim at one fixed end: the first press worked and every later one tweened
-- yellow to yellow (or 2pi to 2pi, 2.2 to 2.2), which reads as a dead function.
local lifted = false
local turned = false
local grown = false
local yellow = false

local function subject(i) return Entity.find_by_name("Subject" .. i) end

local function reset_all()
    for i = 1, 4 do
        local e = subject(i)
        if e ~= nil then
            Tween.cancel_all(e)
            Transform.set_position(e, HOME[i], HOME_Y)
            Transform.set_rotation(e, 0.0)
            Transform.set_scale(e, 1.0, 1.0)
            if i == 4 then Sprite.set_color(e, PURPLE[1], PURPLE[2], PURPLE[3], PURPLE[4]) end
        end
    end
    ids = {}
    lifted = false
    turned = false
    grown = false
    yellow = false
end

local function launch(i)
    local e = subject(i)
    if e == nil then return end
    if i == 1 then
        lifted = not lifted
        ids[1] = Tween.position(e, HOME[1], lifted and LIFT_Y or HOME_Y, 1.2, "ease_in_out_quad")
        last = "tween_position(" .. (lifted and "up" or "down") .. ", ease_in_out_quad)"
    elseif i == 2 then
        turned = not turned
        ids[2] = Tween.rotation(e, turned and 6.2832 or 0.0, 1.5, "ease_out_quad")
        last = "tween_rotation(" .. (turned and "2pi" or "0") .. ", ease_out_quad)"
    elseif i == 3 then
        grown = not grown
        local k = grown and 2.2 or 1.0
        ids[3] = Tween.scale(e, k, k, 0.9, "ease_in_quad")
        last = "tween_scale(" .. (grown and "2.2" or "1") .. ", ease_in_quad)"
    elseif i == 4 then
        yellow = not yellow
        local to = yellow and YELLOW or PURPLE
        ids[4] = Tween.color(e, to[1], to[2], to[3], to[4], 1.0, "linear")
        last = "tween_color(" .. (yellow and "yellow" or "purple") .. ", linear)"
    end
end

function init(entity)
    log("[scene_49] ready - 1-4 launch a tween, A all, C cancel, R reset")
end

function update(entity, dt)
    for i = 1, 4 do
        if Input.is_key_just_pressed(tostring(i)) then launch(i) end
    end
    if Input.is_key_just_pressed("A") then
        for i = 1, 4 do launch(i) end
        last = "all four at once"
    elseif Input.is_key_just_pressed("C") then
        -- cancel_tween takes the id returned at launch; cancel_all_tweens
        -- clears every tween on an entity regardless of who started it.
        for i = 1, 4 do
            if ids[i] ~= nil then Tween.cancel(ids[i]) end
        end
        ids = {}
        last = "cancel_tween on each id"
    elseif Input.is_key_just_pressed("R") then
        reset_all()
        last = "reset (cancel_all_tweens + restore)"
    end

    local r = Entity.find_by_name("Readout")
    if r then Text.set(r, "last: " .. last) end
    nav.tick(SCENE_PATH)
end
