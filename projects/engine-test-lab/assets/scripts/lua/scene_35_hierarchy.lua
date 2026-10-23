-- Scene 35 - Hierarchy & Parenting
--
-- Everything visible here is authored in the scene: `parent` is an entity-level
-- field (a sibling of `components`, not a component), and the hierarchy
-- resolver composes each child's local Transform with its parent's to produce
-- a WorldTransform. Only the spinner has a velocity - the satellites, the moon
-- and the scaled child move purely because their ancestors do.
--
-- The script covers the two script-facing halves: reading composed positions
-- with Transform.get_world_position(), and re-binding a parent at runtime with
-- Entity.set_parent().

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/35-hierarchy.scene"

local spinner, moon, floater, readout
local attached = false

function init(entity)
    spinner = Entity.find_by_name("Spinner")
    moon = Entity.find_by_name("Moon")
    floater = Entity.find_by_name("Floater")
    readout = Entity.find_by_name("Readout")
    log("[scene_35] ready - Space attaches/detaches the green box from the spinner")
end

function update(entity, dt)
    if Input.is_key_just_pressed("Space") and floater then
        attached = not attached
        if attached then
            -- The box keeps its local Transform, so it snaps to the spinner's
            -- frame rather than staying where it was on screen. That is the
            -- point: local coordinates are relative to the parent.
            Entity.set_parent(floater, spinner)
        else
            Entity.set_parent(floater, nil)
        end
    end

    if readout and moon then
        -- Local position of the moon is a fixed (48, 0); the world position
        -- below moves, because two ancestors are rotating.
        -- All three world getters: position composes translation, rotation
        -- composes the parents' angles, and scale multiplies down the chain.
        -- The moon's LOCAL transform never changes; every number here moves
        -- because two ancestors are turning.
        local wx, wy = Transform.get_world_position(moon)
        local wrot = Transform.get_world_rotation(moon)
        local wsx, wsy = Transform.get_world_scale(moon)
        local lrot = Transform.get_rotation(moon)
        local lsx, lsy = Transform.get_scale(moon)
        local state = attached and "child of Spinner" or "detached (world root)"
        Text.set(readout, string.format(
            "Moon world %.0f,%.0f rot %.2f scale %.2f,%.2f   |   local rot %.2f scale %.2f,%.2f   |   box: %s",
            wx or 0, wy or 0, wrot or 0, wsx or 1, wsy or 1,
            lrot or 0, lsx or 1, lsy or 1, state))
    end

    nav.tick(SCENE_PATH)
end
