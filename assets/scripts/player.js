// Player movement script (JavaScript)
// Bounces the entity up and down

var time = 0;

function init(entity) {
    log("JS player script initialized for entity " + entity);
}

function update(entity, dt) {
    time += dt;

    // Get current position
    var pos = get_position(entity);

    // Bounce vertically
    var new_y = Math.sin(time * 3.0) * 80.0;
    set_position(entity, pos[0], new_y);

    // Rotate in opposite direction
    set_rotation(entity, -time * 2.0);
}
