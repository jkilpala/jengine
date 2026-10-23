-- Scene 99: a textured mesh, drawn in the sprite pass.
--
-- `Mesh2D` is a textured polygon: a sprite with the corners free. It sorts
-- with the sprites by `z_order` - which is why the red mesh on the right can
-- be *under* the blue sprite - and its vertices carry a perspective weight
-- `q`, which is what keeps the middle trapezoid's rows straight while its
-- twin's bend along the diagonal.
--
-- Everything here is authored in the scene. There is no script setter for a
-- mesh yet; a package writes the component directly (that is how the
-- crawler draws its corridor). This script only turns the flat quad, to
-- show a mesh follows its entity's transform like any sprite.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/99-textured-mesh.scene"

local spinner
local t = 0

function init(entity)
    spinner = Entity.find_by_name("Spinner")
    log("[99-textured-mesh] ready - three runtimes should agree pixel for pixel")
end

function update(entity, dt)
    t = t + dt
    if spinner ~= nil then
        Transform.set_rotation(spinner, t * 0.4)
    end
    nav.tick(SCENE_PATH)
end
