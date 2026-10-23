-- Scene 60 - Project Packages (testlab-spin)
--
-- Scene 59 demonstrates the package system with `jengine-parallax`, which
-- ships with the engine and is compiled into every runtime. That proves the
-- package *host* works. It proves nothing about whether *someone else's*
-- package can run, which is the case that actually matters.
--
-- This scene uses `projects/engine-test-lab/packages/testlab-spin`: a plain
-- Rust crate living inside the project, exactly where a third party's would.
-- It registers a `spin` component and an Animation-phase system that rotates
-- anything carrying it.
--
-- HOW IT GETS LINKED. Packages are Rust crates linked at compile time, so
-- something has to be rebuilt with them:
--
--   Editor:  ./build-project-viewport.ps1 -Project projects/engine-test-lab
--            Generates .jengine/viewport/ (a wrapper crate re-exporting the
--            viewport and registering this project's packages at module init)
--            and builds it over editor/src/lib/viewport-wasm/.
--
--   Native:  a build generates .jengine/runtime/ the same way, so
--            `jengine-builder` compiles that instead of the stock runtime.
--            NOTE: a plain `cargo run -p jengine-runtime` is the STOCK binary
--            and does not link this package, so the bars will not spin there.
--
-- The viewport directory is shared between projects, so it carries whichever
-- project it was last built for. The readout below says whether the package is
-- actually live, rather than leaving you to guess from a still image.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/60-project-packages.scene"

local readout
local watched
local start_rot = nil
local seen_motion = false
local t = 0.0

function init(entity)
    readout = Entity.find_by_name("Readout")
    watched = Entity.find_by_name("SpinFast")
    if watched ~= nil then
        start_rot = Transform.get_rotation(watched)
    end
    log("[scene_60] ready - if the bars spin, a project-local Rust package is running")
end

function update(entity, dt)
    t = t + dt

    if readout == nil then
        nav.tick(SCENE_PATH)
        return
    end

    if watched == nil or start_rot == nil then
        Text.set(readout, "spin: SpinFast entity not found")
        nav.tick(SCENE_PATH)
        return
    end

    local rot = Transform.get_rotation(watched)
    -- Latched: once motion is seen it stays reported, so a bar that happens to
    -- pass its starting angle does not flicker the readout back to "not running".
    if math.abs(rot - start_rot) > 0.05 then
        seen_motion = true
    end

    if seen_motion then
        Text.set(readout, string.format(
            "testlab-spin IS running   |   SpinFast rotation %.2f rad", rot))
    else
        Text.set(readout, string.format(
            "testlab-spin NOT running (%.1fs) - this runtime was not built with the package",
            t))
    end

    nav.tick(SCENE_PATH)
end
