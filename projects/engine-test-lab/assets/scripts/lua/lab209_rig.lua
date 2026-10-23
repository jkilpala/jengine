-- Scene 209's rig: counts the one-shot layers that ended (on_layer_end).

local ends = 0

function on_layer_end(entity)
    ends = ends + 1
    UI.set_text(Entity.find_by_name("Line2"), "on_layer_end heard " .. ends .. " time(s)")
end
