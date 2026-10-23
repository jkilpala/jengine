-- Scene 59 - Packages (parallax)
--
-- Engine features can ship as Rust packages: a crate implementing
-- `JEnginePackage`, which registers
--
--   * components   -> into the ComponentRegistry the scene loader uses
--   * systems      -> into the SystemSchedule, tagged with a phase + priority
--   * script API   -> metadata for editor tooltips and the generated API index
--   * editor bits  -> inspector fields, menus, toolbar actions
--
-- The runtime owns a `PackageHost` holding all of that and calls `run_phase`
-- at each boundary of its frame, so a package's system runs where its phase
-- says it will.
--
-- WHY THIS SCENE IS v2: package components cannot appear in a v1 scene at all.
-- v1 deserializes into `ComponentBundle`, a fixed struct in core with no field
-- for anyone else's component. v2 dispatches every key through the
-- ComponentRegistry, which is exactly where a package registers - so `.scene`
-- files that use package components must say `"version": 2`.
--
-- This scene uses `jengine-parallax`, deliberately: it is the one built-in
-- package with no hardcoded path in any runtime, so it only moves if the
-- package system is genuinely running. Physics, audio and particles would
-- have looked fine either way, because every runtime calls those directly.
-- See BUGS.md #13.
--
-- PLATFORM NOTE: packages are Rust crates linked at compile time - there is no
-- dynamic loading. A package runs in the native runtime and in a web build
-- (both compiled per project); it reaches the *editor* only when the viewport
-- WASM is rebuilt with it. The three built-ins here are compiled into the
-- viewport, so this scene works in all three.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/59-packages.scene"

local SPEED = 220.0

local readout
local camera
local far_band
local start_far = nil
local t = 0.0

function init(entity)
    readout = Entity.find_by_name("Readout")
    camera = Entity.find_by_name("Camera")
    far_band = Entity.find_by_name("FarBand")
    if far_band ~= nil then
        start_far = Transform.get_position(far_band)
    end
    log("[scene_59] ready - A/D moves the camera; bands track it at 0.15 / 0.45 / 0.85")
end

function update(entity, dt)
    t = t + dt

    -- Move the camera directly; the parallax system reacts to wherever the
    -- active camera ends up, so nothing here touches the bands.
    if camera ~= nil then
        local dx = 0.0
        if Input.is_key_pressed("A") or Input.is_key_pressed("Left") then dx = dx - 1.0 end
        if Input.is_key_pressed("D") or Input.is_key_pressed("Right") then dx = dx + 1.0 end
        if dx ~= 0.0 then
            local cx, cy = Transform.get_position(camera)
            Transform.set_position(camera, cx + dx * SPEED * dt, cy)
        end
    end

    if readout ~= nil then
        local cx = camera and Transform.get_position(camera) or 0.0
        if far_band ~= nil and start_far ~= nil then
            local fx = Transform.get_position(far_band)
            -- If the package system is not running, the band never leaves its
            -- authored x no matter where the camera goes.
            local live = math.abs(fx - start_far) > 0.5 or math.abs(cx) < 0.5
            Text.set(readout, string.format(
                "camera x %.0f   far band x %.0f   %s",
                cx, fx,
                live and "parallax package running"
                     or "band frozen - package system not running"))
        else
            Text.set(readout, string.format("camera x %.0f", cx))
        end
    end

    nav.tick(SCENE_PATH)
end
