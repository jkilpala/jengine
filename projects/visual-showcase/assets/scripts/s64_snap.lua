-- 64 Storming the Gate: a plank (or the far post) whose hinge snapped
-- tells the driver - `on_joint_break` is called on the joint's own entity.

function on_joint_break(entity)
    Event.send("plank_snapped", entity)
end
