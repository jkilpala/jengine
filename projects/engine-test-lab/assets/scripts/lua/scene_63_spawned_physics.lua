-- Scene 63 - Physics on SCRIPT-SPAWNED bodies
--
-- Scene 56 drives bodies the *scene* authored. This one drives bodies a
-- *script* created, which is a different code path and the one that was broken.
--
-- `spawn_prefab` returns a script-allocated handle, not an entity id. The host
-- keeps a table mapping one to the other, and physics bodies are keyed by the
-- entity id. For an authored entity the two numbers usually coincide, so every
-- command appears to work; for a spawned one they do not, and the editor spent
-- months pushing the wrong body or none at all (BUGS #43, #45).
--
-- The authored "Control" crate is here as exactly that: the same commands, on a
-- body whose handle happens to be its entity id. If Control responds and the
-- spawned crate does not, the difference is handle resolution, not physics.
--
-- What each key checks, and what was wrong with it:
--
--   [S] spawn_prefab  -- makes a body whose handle /= entity id
--   [I] apply_impulse -- BUGS #43: editor passed the raw handle to the backend
--   [F] apply_force   -- BUGS #43, same
--   [V] set_velocity  -- BUGS #45: the ECS half resolved, the physics half did not
--   [R] set_angular_velocity -- BUGS #43, same
--   [1] Physics.set_body_type(e, "static")     -- BUGS #42: the editor wrote a component
--   [2] Physics.set_body_type(e, "Static")     -- BUGS #41: capitalisation differed
--   [3] Physics.set_body_type(e, "dynamic")
--   [G] set_gravity                    -- BUGS #46: the missing-argument default differed
--   [C] set_sprite_corners             -- BUGS #44: inert on web for spawned entities
--   [D] destroy_entity on the newest crate, then [S] again -- BUGS #53: the body
--       was never removed. The despawned crate left an invisible collider
--       behind, and the next spawn reused its entity index and was handed the
--       dead crate's body: it appeared where the old crate lay instead of in
--       the spawn row. Now it must vanish outright and the respawn must drop
--       from the row like any other.
--
-- A crate that ignores a key is one of those coming back.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/63-spawned-physics.scene"

-- Both crates are authored `mass: 1.0`, and since Sep 2026 that is what they
-- actually weigh: `RigidBody2D.mass` used to be read by nothing and rapier
-- derived mass from collider density x area, so a 40x40 crate weighed ~1600 and
-- an impulse sized for it did nothing visible (BUGS #50).
--
-- PUSH is a force, so it is an acceleration of 1500 units/s^2 against gravity's
-- 981. IMPULSE is a velocity change straight up, so 620 clears about 200 units.
local PUSH = 1500.0
local IMPULSE = 620.0

local crates = {}        -- script handles from spawn_prefab
local control = nil      -- the authored crate, for comparison
local last = "-"
local gravity_on = true
local spawn_x = 220.0

local function newest()
    if #crates == 0 then return nil end
    return crates[#crates]
end

-- Apply the same command to the spawned crate and to the authored control, so
-- a divergence between them is visible in one keypress rather than two runs.
local function both(fn)
    local e = newest()
    if e ~= nil then fn(e) end
    if control ~= nil then fn(control) end
end

function init(entity)
    control = Entity.find_by_name("Control")
    log("[scene_63] ready - S spawns a crate, then I/F/V/R push it. Compare with the blue control crate.")
end

function update(entity, dt)
    if Input.is_key_just_pressed("S") then
        -- Spawn along a row so several crates are visible at once.
        local e = Entity.spawn_prefab("PhysCrate")
        if e ~= nil then
            crates[#crates + 1] = e
            Transform.set_position(e, spawn_x, 120.0)
            spawn_x = spawn_x + 52.0
            if spawn_x > 520.0 then spawn_x = 220.0 end
            last = string.format("spawned handle %s", tostring(e))
        else
            last = "spawn_prefab returned nil"
        end
    end

    if Input.is_key_just_pressed("D") then
        local e = newest()
        if e ~= nil then
            Entity.destroy(e)
            crates[#crates] = nil
            last = "despawned the newest crate - nothing may be left where it was"
        else
            last = "spawn a crate first"
        end
    end

    if Input.is_key_pressed("F") then
        both(function(e) Physics.apply_force(e, PUSH, 0.0) end)
        last = "apply_force +x"
    end
    if Input.is_key_just_pressed("I") then
        both(function(e) Physics.apply_impulse(e, 0.0, IMPULSE) end)
        last = "apply_impulse +y"
    end
    if Input.is_key_just_pressed("V") then
        both(function(e) Physics.set_velocity(e, -180.0, 220.0) end)
        last = "set_velocity (-180, 220)"
    end
    if Input.is_key_just_pressed("R") then
        both(function(e) Physics.set_angular_velocity(e, 9.0) end)
        last = "set_angular_velocity 9.0"
    end

    -- Lower-case and capitalised spellings must behave identically. Before the
    -- fix, "Static" worked in the editor and produced a *dynamic* body in a
    -- shipped build - a wall that falls.
    if Input.is_key_just_pressed("1") then
        both(function(e) Physics.set_body_type(e, "static") end)
        last = 'set_body_type "static" - crates should freeze'
    end
    if Input.is_key_just_pressed("2") then
        both(function(e) Physics.set_body_type(e, "Static") end)
        last = 'set_body_type "Static" - identical to [1] if the fix holds'
    end
    if Input.is_key_just_pressed("3") then
        both(function(e) Physics.set_body_type(e, "dynamic") end)
        last = 'set_body_type "dynamic" - crates should fall again'
    end

    if Input.is_key_just_pressed("G") then
        gravity_on = not gravity_on
        -- Both components always passed: the runtimes disagreed about what a
        -- missing one meant, and the docs now say it is required.
        if gravity_on then
            Physics.set_gravity(0.0, -9.81)
            last = "gravity on"
        else
            Physics.set_gravity(0.0, 0.0)
            last = "gravity off"
        end
    end

    if Input.is_key_just_pressed("C") then
        -- On a spawned entity this is the case that was inert on web.
        local e = newest()
        if e ~= nil then
            Sprite.set_corners(e, -30, -20, 30, -20, 18, 20, -18, 20)
            last = "set_sprite_corners on the spawned crate"
        else
            last = "spawn a crate first"
        end
    end

    Text.set(Entity.find_by_name("Readout"), string.format("crates: %d", #crates))
    Text.set(Entity.find_by_name("Readout2"), string.format("last: %s", last))

    local e = newest()
    if e ~= nil then
        local _, y = Transform.get_position(e)
        Text.set(Entity.find_by_name("Readout3"), string.format("spawned y (by handle): %.1f", y or 0.0))
        -- Same crate, reached by name instead of by the spawn handle. The two
        -- must agree. They are shown separately because they take different
        -- routes: a handle goes through the preamble's alias map (script index
        -- -> entity id) and a name does not, so a disagreement points straight
        -- at the alias rather than at the mirror or at physics.
        local byname = Entity.find_by_name("PhysCrate")
        if byname ~= nil then
            local _, ny = Transform.get_position(byname)
            Text.set(Entity.find_by_name("Readout5"),
                string.format("spawned y (by name):   %.1f", ny or 0.0))
        else
            Text.set(Entity.find_by_name("Readout5"), "by name: not found")
        end
    else
        Text.set(Entity.find_by_name("Readout3"), "spawned y: - (press S)")
        Text.set(Entity.find_by_name("Readout5"), "by name: -")
    end
    if control ~= nil then
        local _, cy = Transform.get_position(control)
        Text.set(Entity.find_by_name("Readout4"), string.format("control y: %.1f", cy or 0.0))
    end

    nav.tick(SCENE_PATH)
end
