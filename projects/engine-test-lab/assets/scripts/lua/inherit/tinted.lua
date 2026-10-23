-- A module that extends another: everything the spinner has, plus a tint
-- laid on in `init` before handing on to the spinner's own `init`.
script = { extends = "lua/inherit/spinner" }

local M = {}

function M.init(entity)
    Sprite.set_color(entity, 1.0, 0.6, 0.2, 1.0)
    super.init(entity)
end

return M
