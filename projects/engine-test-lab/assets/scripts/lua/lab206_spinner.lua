-- Scene 206's rooms: a script that came in with a streamed chunk. It
-- turns its arrow, which is only true if the chunk's script started.

-- One file serves every room's arrow, so each keeps its own clock.
local clock = {}
function update(entity, dt)
    clock[entity] = (clock[entity] or 0) + dt
    Transform.set_rotation(entity, clock[entity] * 3)
end
