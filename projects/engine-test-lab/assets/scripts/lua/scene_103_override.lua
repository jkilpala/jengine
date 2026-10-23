-- Scene 103, middle: its own `update`, which calls the spinner's through
-- `super` and then bobs. It should spin *and* bob; spinning without bobbing
-- means the override was ignored, bobbing without spinning means `super`
-- does not reach the parent.
script = { extends = "lua/inherit/spinner" }

local t = 0
local base_y = nil

function update(entity, dt)
    super.update(entity, dt)
    local x, y = Transform.get_position(entity)
    if base_y == nil then base_y = y end
    t = t + dt
    Transform.set_position(entity, x, base_y + math.sin(t * 2) * 40)
end
