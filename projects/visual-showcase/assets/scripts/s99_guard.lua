-- 99 Night Watch: a guard. Its behaviour is its `behavior_tree`; this only
-- answers the tree's `call` leaf, `catch` - the driver sends the thief home.

script = { tick_rate = 60 }

function on_catch(entity)
    Event.send("thief_caught", entity, { guard = entity })
    Entity.set_custom(entity, "Blackboard", "catch", "success")
end
