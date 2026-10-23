-- HUD Counter Script
-- Updates the text to show the current bunny count.
-- Attach to a Text entity.

local update_timer = 0

function init(entity)
    log("BunnyMark counter initialized")
end

function update(entity, dt)
    -- Update display every 10 frames (~6 times per second) to avoid overhead
    update_timer = update_timer + dt
    if update_timer < 0.15 then
        return
    end
    update_timer = 0

    -- Find the spawner to read the total count
    local spawner = Entity.find_by_name("Spawner")
    if spawner then
        local total = Entity.get_custom(spawner, "BunnySpawner", "total_spawned") or 0
        -- Note: set_text is not in API yet, so we update via custom component workaround
        -- For now the spawner logs the count to console
    end
end
