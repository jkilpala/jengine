-- Scene 22: Vector Paths
-- Showcases `VectorPath` component (lyon-tessellated 2D shapes) with
-- MoveTo / LineTo / QuadTo / CubicTo / Close commands and fill +
-- stroke colors. Rendered via `render_shapes_wgpu` -> WgpuShapeBatch.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/22-vector-paths.scene"

function init(entity)
    log("[22-vector-paths] ready - lyon tessellation showcase")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
