-- BunnyMark Spawner Script
-- Spawns bunnies on click or Space key press.
-- Attach to an entity with the BunnySpawner custom component.

-- Tint colors for visual variety (RGBA)
local TINTS = {
    {1.0, 1.0, 1.0, 1.0},   -- white
    {1.0, 0.4, 0.4, 1.0},   -- red
    {0.4, 1.0, 0.4, 1.0},   -- green
    {0.4, 0.4, 1.0, 1.0},   -- blue
    {1.0, 1.0, 0.4, 1.0},   -- yellow
    {1.0, 0.4, 1.0, 1.0},   -- magenta
    {0.4, 1.0, 1.0, 1.0},   -- cyan
    {1.0, 0.7, 0.3, 1.0},   -- orange
}

local tint_counter = 0
local spawn_timer = 0

function init(entity)
    log("BunnyMark spawner ready — click or press SPACE to spawn bunnies!")
end

function update(entity, dt)
    local spawn_count = Entity.get_custom(entity, "BunnySpawner", "spawn_count") or 100
    local auto_spawn = Entity.get_custom(entity, "BunnySpawner", "auto_spawn")

    -- Spawn bunnies on click or Space
    local should_spawn = Input.is_key_just_pressed("Space")

    -- Auto-spawn mode: add bunnies every second
    if auto_spawn then
        spawn_timer = spawn_timer + dt
        if spawn_timer >= 1.0 then
            spawn_timer = spawn_timer - 1.0
            should_spawn = true
        end
    end

    if should_spawn then
        for i = 1, spawn_count do
            local bunny = Entity.spawn_prefab("Bunny")
            if bunny ~= 0 then
                -- Spawn from top-center with random velocity
                local vx = (math.random() - 0.5) * 500
                local vy = math.random() * 150 - 50

                Transform.set_position(bunny, 0, 320)
                Entity.set_custom(bunny, "BunnyData", "velocity_x", vx)
                Entity.set_custom(bunny, "BunnyData", "velocity_y", vy)

                -- Cycle through tint colors
                tint_counter = tint_counter + 1
                local tint = TINTS[((tint_counter - 1) % #TINTS) + 1]
                Sprite.set_color(bunny, tint[1], tint[2], tint[3], tint[4])
                Entity.set_custom(bunny, "BunnyData", "tint_index", (tint_counter - 1) % #TINTS)
            end
        end

        local total = (Entity.get_custom(entity, "BunnySpawner", "total_spawned") or 0) + spawn_count
        Entity.set_custom(entity, "BunnySpawner", "total_spawned", total)
        log("Spawned " .. spawn_count .. " bunnies (total: " .. total .. ")")
    end
end
