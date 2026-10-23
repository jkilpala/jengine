// Bunny Movement Script
// Applied to each bunny entity via the Bunny prefab.
// Simulates gravity, bouncing off walls and floor.

var GRAVITY = 500;          // pixels/sec^2, pulling downward
var HALF_WIDTH = 640;       // screen half-width
var HALF_HEIGHT = 360;      // screen half-height
var BOUNCE_DAMPING = 0.9;   // energy loss on bounce

function init(entity) {
    // Nothing to do — velocity is set by the spawner
}

function update(entity, dt) {
    // Read velocity from custom component
    var vx = get_custom(entity, "BunnyData", "velocity_x") || 0;
    var vy = get_custom(entity, "BunnyData", "velocity_y") || 0;

    // Apply gravity (downward in +Y up coordinate system)
    vy = vy - GRAVITY * dt;

    // Get current position
    var pos = get_position(entity);
    var x = pos[0];
    var y = pos[1];

    // Move
    x = x + vx * dt;
    y = y + vy * dt;

    // Bounce off horizontal walls
    if (x < -HALF_WIDTH) {
        x = -HALF_WIDTH;
        vx = Math.abs(vx) * BOUNCE_DAMPING;
    } else if (x > HALF_WIDTH) {
        x = HALF_WIDTH;
        vx = -Math.abs(vx) * BOUNCE_DAMPING;
    }

    // Bounce off floor
    if (y < -HALF_HEIGHT) {
        y = -HALF_HEIGHT;
        vy = Math.abs(vy) * BOUNCE_DAMPING;
        // Add random horizontal perturbation
        vx = vx + (Math.random() - 0.5) * 60;
    }

    // Bounce off ceiling
    if (y > HALF_HEIGHT) {
        y = HALF_HEIGHT;
        vy = -Math.abs(vy) * BOUNCE_DAMPING;
    }

    // Write back
    set_position(entity, x, y);
    set_custom(entity, "BunnyData", "velocity_x", vx);
    set_custom(entity, "BunnyData", "velocity_y", vy);
}
