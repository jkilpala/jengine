// Scene 58 — batch script (tier 3).
//
// One `update(ctx, q)` call moves EVERY entity carrying this
// `BatchScriptComponent`, instead of one `update(entity, dt)` call per entity.
// The columns are `Float32Array` views over mirror buffers the dispatcher
// gathers from the ECS before the call and scatters back after, so the whole
// batch costs a handful of host crossings regardless of entity count.
//
// The `system.query` schema below is validated at load time against the
// dispatcher's contract — a script asking for a column the runner does not
// gather is rejected rather than silently receiving zeroes.
//
// Reads:  Acceleration2D, Drag2D
// Writes: Velocity2D, Position2D
//
// Velocity2D is a *write* column that the dispatcher pre-fills with the
// current ECS value, which is what makes read-modify-write integration work.
const system = {
    query: {
        read: ["Acceleration2D", "Drag2D"],
        write: ["Velocity2D", "Position2D"],
    },
    tick_rate: 60,
};

function update(ctx, q) {
    const ax = q.read.Acceleration2D.x;
    const ay = q.read.Acceleration2D.y;
    const dx = q.read.Drag2D.x;
    const dy = q.read.Drag2D.y;
    const vx = q.write.Velocity2D.x;
    const vy = q.write.Velocity2D.y;
    const px = q.write.Position2D.x;
    const py = q.write.Position2D.y;
    const dt = ctx.dt;
    const n = q.count;
    for (let i = 0; i < n; i++) {
        // drag + force integrator, the canonical batch shape
        vx[i] = vx[i] * (1 - dx[i]) + ax[i] * dt;
        vy[i] = vy[i] * (1 - dy[i]) + ay[i] * dt;
        px[i] = px[i] + vx[i] * dt;
        py[i] = py[i] + vy[i] * dt;
        // Bounce off a box so the drifters stay on screen without needing a
        // second (per-entity) script to police them.
        if (px[i] < -420 || px[i] > 420) vx[i] = -vx[i];
        if (py[i] < -200 || py[i] > 140) vy[i] = -vy[i];
    }
}
