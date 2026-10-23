-- Scene 183: a chain link or weight whose joint snapped tells the
-- controller (`on_joint_break` is called on the joint's own entity).

function on_joint_break(entity)
    Event.send("snapped", entity)
end
