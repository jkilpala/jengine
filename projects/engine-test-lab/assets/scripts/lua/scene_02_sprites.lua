-- Scene 02: Sprites
-- Visualises tint, anchor, z-order, blend modes, uv_rect. No animation,
-- just nav passthrough.

script = { tick_rate = 10 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/02-sprites.scene"

function init(entity)
    log("[02-sprites] ready")
end

-- set_sprite_corners tints the four corners independently (a cheap gradient
-- without a shader); get_sprite_point resolves a named attachment point on the
-- sprite to world coordinates. The latter is listed as not-yet-on-bridge, so
-- the call is guarded rather than assumed.
local corner_t = 0.0
local corner_target = nil

function update(entity, dt)
    corner_t = corner_t + dt
    if corner_target == nil then corner_target = Entity.find_by_name("BlendAlpha") or Entity.find_by_name("AnchorCenter") end
    if corner_target ~= nil then
        local p = 0.5 + 0.5 * math.sin(corner_t)
        Sprite.set_corners(corner_target,
            1.0, 0.3, 0.3, 1.0,
            0.3, 1.0, 0.3, 1.0,
            0.3, 0.3, 1.0, 1.0,
            p, p, 0.2, 1.0)
        if Sprite.get_point ~= nil then
            local _px, _py = Sprite.get_point(corner_target, "muzzle")
        end
    end
    nav.tick(SCENE_PATH)
end
