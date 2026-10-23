-- Scene 06: Animations
--
-- Demonstrates the SpriteAnimator + property keyframe pipeline:
--   * UvCycle  — 4-frame uv_rect cycle on the npc_villager spritesheet
--   * Rotate   — single-frame clip with a Rotation property track (0 → 2π)
--   * Pulse    — ColorR/G/B property tracks cycling RGB tint
--   * Bounce   — PositionY + ScaleX/ScaleY tracks (hop + squash/stretch)
--
-- Scripts drive playback controls only (the animators auto-play on scene
-- load via `"playing": true` in the .scene file):
--   SPACE = pause/resume all
--   1     = speed x0.5
--   2     = speed x1.0
--   3     = speed x2.0
--   R     = restart (replay clip from frame 0)

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/06-animations.scene"

local TARGETS = { "UvCycle", "Rotate", "Pulse", "Bounce" }
local CLIPS   = { UvCycle = "walk", Rotate = "spin", Pulse = "pulse", Bounce = "bounce" }

local cached_ids = nil
local playing = true
local prev_space, prev_1, prev_2, prev_3, prev_r = false, false, false, false, false

local function ids()
    if cached_ids then return cached_ids end
    local ok = true
    local out = {}
    for _, name in ipairs(TARGETS) do
        local id = Entity.find_by_name(name)
        if not id then ok = false; break end
        out[name] = id
    end
    if ok then cached_ids = out end
    return out
end

local function for_each(fn)
    local map = ids()
    for _, name in ipairs(TARGETS) do
        local id = map[name]
        if id then fn(id, name) end
    end
end

local function set_speed_all(s)
    for_each(function(id) Animation.set_speed(id, s) end)
    log(string.format("[06-animations] speed = x%.1f", s))
end

local function toggle_pause()
    if playing then
        for_each(function(id) Animation.stop(id) end)
        playing = false
        log("[06-animations] paused")
    else
        for_each(function(id, name) Animation.play(id, CLIPS[name]) end)
        playing = true
        log("[06-animations] resumed")
    end
end

local function restart_all()
    for_each(function(id, name) Animation.play(id, CLIPS[name]) end)
    playing = true
    log("[06-animations] restarted")
end

function init(entity)
    log("[06-animations] ready - SPACE pause, 1/2/3 speed, R restart")
end

-- set_animation_time / get_animation_time scrub a clip directly instead of
-- letting it advance, which is what a timeline scrubber or a hit-frame lookup
-- needs. Hold S to scrub with the mouse X position.
local scrub_target = nil

function update(entity, dt)
    if scrub_target == nil then scrub_target = Entity.find_by_name("Villager") or Entity.find_by_name("UvCycle") end
    if scrub_target ~= nil and Input.is_key_pressed("S") then
        local mx, _ = Input.get_mouse_position()
        local sw, _ = Camera.get_screen_size()
        Animation.set_time(scrub_target, (mx / math.max(1, sw)) * 1.0)
    end
    nav.tick(SCENE_PATH)

    local k_space = Input.is_key_pressed("Space")
    local k1 = Input.is_key_pressed("1")
    local k2 = Input.is_key_pressed("2")
    local k3 = Input.is_key_pressed("3")
    local kr = Input.is_key_pressed("R")

    if k_space and not prev_space then toggle_pause() end
    if k1 and not prev_1 then set_speed_all(0.5) end
    if k2 and not prev_2 then set_speed_all(1.0) end
    if k3 and not prev_3 then set_speed_all(2.0) end
    if kr and not prev_r then restart_all() end

    prev_space, prev_1, prev_2, prev_3, prev_r = k_space, k1, k2, k3, kr
end
