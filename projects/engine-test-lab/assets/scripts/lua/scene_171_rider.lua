-- Scene 171: the runner. Its `path_follow` is `once`; the engine calls
-- on_path_end when it gets there, and this sends it round again.

local wait = 0

function on_path_end(entity)
    wait = 0.8
    Sprite.set_color(entity, 1.0, 0.75, 0.2, 1)
    Event.send("arrived", entity, { at = "Spur" })
end

function update(entity, dt)
    if wait > 0 then
        wait = wait - dt
        if wait <= 0 then
            Sprite.set_color(entity, 1, 1, 1, 1)
            PathFollow.set(entity, "progress", 0)
            PathFollow.set(entity, "playing", true)
        end
    end
end
