-- Scene 170: one lantern. The engine calls these when its
-- `visibility_notifier` comes into view or leaves it.

function on_screen_entered(entity)
    Sprite.set_color(entity, 1.0, 0.82, 0.3, 1)
    Event.send("lantern_seen", entity, { d = 1 })
end

function on_screen_exited(entity)
    Sprite.set_color(entity, 0.3, 0.32, 0.4, 1)
    Event.send("lantern_seen", entity, { d = -1 })
end
