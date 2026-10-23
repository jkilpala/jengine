// BunnyMark Spawner Script (JavaScript)
// Spawns bunnies on Space key press.
// Attach to an entity with the BunnySpawner custom component.

// Tint colors for visual variety [R, G, B, A]
var TINTS = [
    [1.0, 1.0, 1.0, 1.0],   // white
    [1.0, 0.4, 0.4, 1.0],   // red
    [0.4, 1.0, 0.4, 1.0],   // green
    [0.4, 0.4, 1.0, 1.0],   // blue
    [1.0, 1.0, 0.4, 1.0],   // yellow
    [1.0, 0.4, 1.0, 1.0],   // magenta
    [0.4, 1.0, 1.0, 1.0],   // cyan
    [1.0, 0.7, 0.3, 1.0],   // orange
];

var tint_counter = 0;
var spawn_timer = 0;

function init(entity) {
    log("BunnyMark spawner ready — click or press SPACE to spawn bunnies!");
}

function update(entity, dt) {
    var spawn_count = get_custom(entity, "BunnySpawner", "spawn_count") || 100;
    var auto_spawn = get_custom(entity, "BunnySpawner", "auto_spawn");

    // Spawn bunnies on Space
    var should_spawn = is_key_just_pressed("Space");

    // Auto-spawn mode: add bunnies every second
    if (auto_spawn) {
        spawn_timer = spawn_timer + dt;
        if (spawn_timer >= 1.0) {
            spawn_timer = spawn_timer - 1.0;
            should_spawn = true;
        }
    }

    if (should_spawn) {
        for (var i = 0; i < spawn_count; i++) {
            var bunny = spawn_prefab("Bunny");
            if (bunny !== 0) {
                // Spawn from top-center with random velocity
                var vx = (Math.random() - 0.5) * 500;
                var vy = Math.random() * 150 - 50;

                set_position(bunny, 0, 320);
                set_custom(bunny, "BunnyData", "velocity_x", vx);
                set_custom(bunny, "BunnyData", "velocity_y", vy);

                // Cycle through tint colors
                var tint = TINTS[tint_counter % TINTS.length];
                set_sprite_color(bunny, tint[0], tint[1], tint[2], tint[3]);
                set_custom(bunny, "BunnyData", "tint_index", tint_counter % TINTS.length);
                tint_counter++;
            }
        }

        var total = (get_custom(entity, "BunnySpawner", "total_spawned") || 0) + spawn_count;
        set_custom(entity, "BunnySpawner", "total_spawned", total);
        log("Spawned " + spawn_count + " bunnies (total: " + total + ")");
    }
}
