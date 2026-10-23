-- Scene 247: the day_cycle's own script - told at dawn and dusk.

local dawns, dusks = 0, 0

function init(entity)
    Entity.set_custom(entity, "Counts", "dawns", 0)
    Entity.set_custom(entity, "Counts", "dusks", 0)
end

function on_dawn(entity)
    dawns = dawns + 1
    Entity.set_custom(entity, "Counts", "dawns", dawns)
end

function on_dusk(entity)
    dusks = dusks + 1
    Entity.set_custom(entity, "Counts", "dusks", dusks)
end
