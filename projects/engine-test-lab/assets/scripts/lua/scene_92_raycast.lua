-- Scene 92 - raycast
--
-- A fan of rays from the middle of the screen, each drawn to whatever it
-- hits. A dot marks the hit point and a short spur shows the surface
-- normal, because the normal is the half of the answer a vision cone does
-- not need and a bouncing projectile does.
--
-- This function is worth a scene for two reasons beyond the obvious.
--
-- **It was the last one missing on native.** Everything else the docs
-- listed as absent turned out to be bound; this one really was not, and
-- the reason is that it is the only script read that cannot come from a
-- snapshot. Every other read is a lookup the host can copy once a frame. A
-- ray is not: its origin and direction come from the script, so there is
-- nothing to precompute. Native now lends the live physics world to the
-- script tick (`BridgeReadSnapshot::physics_ptr`), exactly as it lends the
-- ECS for `get_custom`.
--
-- **And it was the standing example of the other kind of gap.** In the
-- editor `raycast` has always been in `BRIDGE_FUNCTIONS`, so every
-- name-level parity check counted it as present - and it read a
-- `__jengine_play.state.physics_raycast` hook that no host ever installed,
-- so it returned `null` in silence for the whole life of the function. A
-- name that binds proves nothing about whether anything answers. The
-- browser hosts install it now, over a snapshot of the colliders that
-- `raycast_snapshot_matches_rapier` holds to rapier's own answers.
--
-- So this scene is the check that all four runtimes agree. Run it in
-- native, the editor and a web build and the fans should look identical -
-- and if one of them draws every ray at full length, that runtime is the
-- one answering nothing.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/92-raycast.scene"

local RAYS = 48
local REACH = 900
local SWEEP_RATE = 0.35

local readout
local t = 0
local spin = 0
local sweeping = true
-- Logged once, a second in. The screen tells a person which runtime is
-- answering nothing; the log tells whoever is reading a sweep's output
-- afterwards, which is the only way an unattended run says anything at all.
local reported = false

local function ray_color(hit)
    if hit == nil then
        return 0.28, 0.30, 0.38
    end
    return 1.0, 0.72, 0.30
end

function init(entity)
    readout = Entity.find_by_name("Readout")
    log("[scene_92] ready - every ray should stop at something, not at its own length")
end

function update(entity, dt)
    t = t + dt
    if Input.is_key_just_pressed("Space") then
        sweeping = not sweeping
    end
    if sweeping then
        spin = spin + dt * SWEEP_RATE
    end

    -- A guard, because this is exactly the function that used to be nil in
    -- one runtime and present in another. If it is ever missing again the
    -- scene should say so rather than erroring out of the whole script.
    if Physics.raycast == nil then
        if readout ~= nil then
            Text.set(readout, "raycast is NOT BOUND in this runtime")
        end
        nav.tick(SCENE_PATH)
        return
    end

    local hits = 0
    local nearest, nearest_dist = nil, REACH * 2

    for i = 0, RAYS - 1 do
        local a = spin + i * (math.pi * 2) / RAYS
        local dx, dy = math.cos(a), math.sin(a)
        local hit = Physics.raycast(0, 0, dx, dy, REACH)
        local r, g, b = ray_color(hit)
        local ex, ey = dx * REACH, dy * REACH
        if hit ~= nil then
            hits = hits + 1
            ex, ey = hit.x, hit.y
            -- The hit point, and a spur along the normal. The normal is
            -- what a ricochet needs and what a vision cone ignores, so it
            -- is worth being able to see that it is right.
            Debug.draw_circle(hit.x, hit.y, 5, 1.0, 0.95, 0.5)
            Debug.draw_line(hit.x, hit.y,
                hit.x + hit.normal_x * 26, hit.y + hit.normal_y * 26,
                0.4, 0.95, 1.0)
            if hit.distance < nearest_dist then
                nearest_dist = hit.distance
                nearest = hit
            end
        end
        Debug.draw_line(0, 0, ex, ey, r, g, b)
    end

    -- One ray under the mouse, drawn brighter, so the scene answers to
    -- something the person is doing rather than only animating.
    local mx, my = Input.get_mouse_world_position()
    local len = math.sqrt(mx * mx + my * my)
    if len > 1 then
        local hit = Physics.raycast(0, 0, mx / len, my / len, REACH)
        local ex, ey = mx / len * REACH, my / len * REACH
        if hit ~= nil then
            ex, ey = hit.x, hit.y
        end
        Debug.draw_line(0, 0, ex, ey, 0.5, 1.0, 0.6)
    end

    if not reported and t > 1.0 then
        reported = true
        if nearest ~= nil then
            log(string.format(
                "[scene_92] %d of %d rays hit; nearest entity %d at %.0f units",
                hits, RAYS, nearest.entity_id, nearest.distance))
        else
            log("[scene_92] NO RAY HIT ANYTHING - this runtime is answering nothing")
        end
    end

    if readout ~= nil then
        if nearest == nil then
            Text.set(readout, string.format(
                "%d of %d rays hit something - if this is 0, this runtime is answering nothing",
                hits, RAYS))
        else
            Text.set(readout, string.format(
                "%d of %d rays hit something   |   nearest: entity %d at %.0f units, "
                .. "point (%.0f, %.0f), normal (%.2f, %.2f)",
                hits, RAYS, nearest.entity_id, nearest.distance,
                nearest.x, nearest.y, nearest.normal_x, nearest.normal_y))
        end
    end

    nav.tick(SCENE_PATH)
end
