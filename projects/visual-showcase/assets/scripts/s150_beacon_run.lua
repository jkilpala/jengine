-- 150 Beacon Run: each beacon carries an `offscreen_marker` (on its own
-- entity), which the engine keeps at the screen's edge pointing at the
-- beacon with its distance, and turns into a flame icon over it once it is
-- in view. The script walks the scout and lights beacons; a lit beacon's
-- marker is switched off with `Marker.set`. Left alone, the scout walks to
-- the nearest unlit beacon by itself.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/150-beacon-run.scene"
local N = 5
local SPEED = 380
local status
local captions = true
local scout, step
local beacons = {}
local x, y = 0, 0
local idle, t, done_t = 99, 0, nil

local function ent_of(n) return Entity.find_by_name(n) end

function init(entity)
    status = Entity.find_by_name("Status")
    scout = ent_of("Scout")
    step = ent_of("ScoutStep")
    for i = 1, N do
        local b = ent_of("Beacon" .. i)
        local bx, by = Transform.get_position(b)
        beacons[i] = { x = bx, y = by, lit = false, litspr = ent_of("Beacon" .. i .. "Lit"),
                       glow = ent_of("Beacon" .. i .. "Glow"), marker = ent_of("Beacon" .. i .. "Marker"),
                       label = ent_of("Beacon" .. i .. "Label") }
    end
end

local function light(b, on)
    b.lit = on
    Sprite.set_color(b.litspr, 1, 1, 1, on and 1 or 0)
    Sprite.set_color(b.glow, 1.0, 0.6, 0.25, on and 0.9 or 0)
    Marker.set(b.marker, "enabled", not on)
    Sprite.set_color(b.marker, 1, 1, 1, on and 0 or 1)
    if on then Text.set(b.label, "") end
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    local dx, dy = 0, 0
    if Input.is_key_pressed("A") or Input.is_key_pressed("Left") then dx = dx - 1 end
    if Input.is_key_pressed("D") or Input.is_key_pressed("Right") then dx = dx + 1 end
    if Input.is_key_pressed("W") or Input.is_key_pressed("Up") then dy = dy + 1 end
    if Input.is_key_pressed("S") or Input.is_key_pressed("Down") then dy = dy - 1 end
    if dx ~= 0 or dy ~= 0 then idle = 0 else idle = idle + dt end
    local lit = 0
    local best, bd = nil, 1e12
    for _, b in ipairs(beacons) do
        if b.lit then lit = lit + 1 else
            local d = (b.x - x) ^ 2 + (b.y - y) ^ 2
            if d < bd then best, bd = b, d end
        end
    end
    if idle > 3 and best ~= nil then
        dx, dy = best.x - x, best.y - y
    end
    local len = math.sqrt(dx * dx + dy * dy)
    if len > 1e-3 then
        x = x + dx / len * SPEED * dt
        y = y + dy / len * SPEED * dt
        local turn = math.atan(dy, dx)
        if math.atan2 ~= nil then turn = math.atan2(dy, dx) end
        Transform.set_rotation(scout, turn)
        Transform.set_rotation(step, turn)
        local a = math.floor(t * 6) % 2
        Sprite.set_color(scout, 1, 1, 1, a == 0 and 1 or 0)
        Sprite.set_color(step, 1, 1, 1, a == 1 and 1 or 0)
    end
    Transform.set_position(scout, x, y)
    Transform.set_position(step, x, y)
    for _, b in ipairs(beacons) do
        if not b.lit and (b.x - x) ^ 2 + (b.y - y) ^ 2 < 70 * 70 then light(b, true) end
    end
    if lit == N then
        done_t = done_t or t
        UI.set_text(status, "every beacon lit")
        if t - done_t > 3 then
            for _, b in ipairs(beacons) do light(b, false) end
            done_t = nil
        end
    else
        UI.set_text(status, lit .. " of " .. N .. " beacons lit" .. (idle > 3 and "   (walking by itself)" or ""))
    end
end
