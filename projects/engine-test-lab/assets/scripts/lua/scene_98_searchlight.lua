-- Scene 98: a searchlight aimed by its transform.
--
-- `SpotLight2D` carries a `direction`, and the wgpu pass writes the cone
-- angle as `entity_rotation + light.direction`. Nothing exercised the first
-- half: scene 28's three spots set `direction` and never turn, so a pass that
-- dropped the transform would have looked perfectly correct there.
--
-- So the left light's `direction` is zero and every degree of its cone comes
-- from `set_rotation` on the entity. The right one is the control - same
-- light, aimed the old way, never rotated.
--
-- The shadows matter as much as the cones: both spots read the same occluder
-- set the point lights use, so the pillar shadows must swing with the beam.
-- Scene 32 covers occlusion for point lights and nothing covered it for spots.
--
-- The CPU twin of all this is `renderer::lighting::spot_light_at`, which a
-- package can call to ask whether a world point is inside a beam. There is no
-- script getter for it - the light setters have never had a reader - so it is
-- pinned by `cpu_light_matches_the_shader` rather than shown here.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/98-searchlight.scene"

-- Radians per second, and how far either side of straight down it swings.
local SWEEP_RATE = 0.55
local SWEEP_ARC = 1.15
-- Straight down. The cone is `rotation + direction` and `direction` is 0 here,
-- so this entity's rotation IS the beam angle.
local CENTRE = -math.pi / 2

local sweeper, readout
local t = 0

function init(entity)
    sweeper = Entity.find_by_name("Sweeper")
    readout = Entity.find_by_name("Readout")
    log("[98-searchlight] ready - the left cone is aimed by Transform.rotation only")
end

function update(entity, dt)
    t = t + dt
    if sweeper ~= nil then
        local angle = CENTRE + math.sin(t * SWEEP_RATE) * SWEEP_ARC
        Transform.set_rotation(sweeper, angle)
        if readout ~= nil then
            -- Floored before `%d`: Luau truncates a fractional argument and
            -- Fengari throws on one, so an unfloored degree count here would
            -- kill this script in the editor and web and look fine natively.
            local deg = math.floor(angle * 180 / math.pi + 0.5)
            Text.set(readout, string.format(
                "sweeper Transform.rotation = %d deg   (SpotLight2D.direction = 0)", deg))
        end
    end
    nav.tick(SCENE_PATH)
end
