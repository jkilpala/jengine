-- Scene 57 - Particle Bursts & Sprite Pivots
--
--   Particles.emit_burst(e, count)     one-shot puff from an existing emitter
--   Particles.get_count()     live particles across the whole system
--   Sprite.set_anchor(e, x, y)  move a sprite's pivot; (0.5,0.5) centres it
--   Sprite.get_anchor(e)        read it back
--
-- Bursts are what an impact or a pickup wants - a continuously running emitter
-- (scene 07) is the wrong shape for a one-off. The particle count is the check
-- that the burst actually reached the system rather than being dropped.
--
-- The anchor half shares this scene because it is one call with an obvious
-- visual: the rotating square turns about whichever pivot is selected, and the
-- red dot marks where that pivot sits.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/57-particle-bursts.scene"

local ANCHORS = {
    { 0.5, 0.5, "centre" },
    { 0.0, 0.0, "bottom-left" },
    { 1.0, 0.0, "bottom-right" },
    { 0.5, 1.0, "top-centre" },
}

local pivot, mark
local anchor_i = 1
local last = "-"

local function apply_anchor(i)
    anchor_i = i
    local a = ANCHORS[i]
    if pivot ~= nil then
        Sprite.set_anchor(pivot, a[1], a[2])
    end
    -- Move the marker to where that pivot actually is: the anchor is a
    -- fraction of the sprite's size measured from its bottom-left.
    if mark ~= nil then
        local px, py = 380.0, 40.0
        local half = 55.0
        Transform.set_position(mark, px + (a[1] - 0.5) * half * 2.0, py + (a[2] - 0.5) * half * 2.0)
    end
    last = string.format("set_sprite_anchor(%.1f, %.1f) - %s", a[1], a[2], a[3])
end

function init(entity)
    pivot = Entity.find_by_name("Pivot")
    mark = Entity.find_by_name("PivotMark")
    apply_anchor(1)
    log("[scene_57] ready - Space bursts both emitters, 1-4 change the pivot")
end

function update(entity, dt)
    if Input.is_key_just_pressed("Space") then
        local f = Entity.find_by_name("Fountain")
        local fi = Entity.find_by_name("Fire")
        if f ~= nil then Particles.emit_burst(f, 120) end
        if fi ~= nil then Particles.emit_burst(fi, 80) end
        last = "emit_burst(fountain, 120) + emit_burst(fire, 80)"
    end
    for i = 1, #ANCHORS do
        if Input.is_key_just_pressed(tostring(i)) then apply_anchor(i) end
    end

    local cl = Entity.find_by_name("CountLabel")
    if cl then
        Text.set(cl, "get_particle_count() = " .. tostring(Particles.get_count() or 0))
    end
    local pl = Entity.find_by_name("PivotLabel")
    if pl then
        local a = ANCHORS[anchor_i]
        Text.set(pl, string.format("anchor (%.1f, %.1f) - %s", a[1], a[2], a[3]))
    end
    local r = Entity.find_by_name("Readout")
    if r then Text.set(r, "last: " .. last) end

    nav.tick(SCENE_PATH)
end
