-- 99 Night Watch: the thief. Its behaviour is its `behavior_tree`; this
-- only answers the `steal` leaf - the driver decides what was in the chest.

script = { tick_rate = 60 }

function on_steal(entity)
    Event.send("thief_steals", entity, { thief = entity })
    Entity.set_custom(entity, "Blackboard", "steal", "success")
end
