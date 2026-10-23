-- 147 Fruit Slicer: every fruit is a `sliceable` body. Each frame the
-- blade has moved, the script cuts along the stretch it moved -
-- `Physics.slice(x0, y0, x1, y1)` - and the engine does the rest: the fruit
-- becomes two bodies drawn with its inside, pushed apart off the cut. Left
-- alone, a ghost blade swipes through the fruit by itself.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/147-fruit-slicer.scene"
local FRUIT = { "FsWatermelon", "FsOrange", "FsApple", "FsKiwi", "FsLemon" }
local COLOURS = { { 1.00, 0.25, 0.30, 0.95 }, { 1.00, 0.60, 0.10, 0.95 }, { 0.95, 0.90, 0.60, 0.95 }, { 0.50, 0.85, 0.20, 0.95 }, { 1.00, 0.90, 0.30, 0.95 } }
local status, blade, juice
local captions = true
local t, toss_in = 0, 0.4
local last_x, last_y
local mouse_x, mouse_y
local idle = 99
local ghost = { from = nil, to = nil, k = 1 }
local cuts, tossed, missed = 0, 0, 0
local known = {}

local function toss()
    local i = math.random(#FRUIT)
    local e = Entity.spawn_prefab(FRUIT[i])
    local x = math.random(-420, 420)
    Transform.set_position(e, x, -400)
    Physics.set_velocity(e, -x * 0.5 + math.random(-120, 120), math.random(820, 980))
    known[e] = i
    tossed = tossed + 1
end

local function count(list)
    local n = 0
    for _ in pairs(list) do n = n + 1 end
    return n
end

function init(entity)
    status = Entity.find_by_name("Status")
    blade = Entity.find_by_name("Blade")
    juice = Entity.find_by_name("Juice")
end

-- A ghost swipe through the highest fruit in the air.
local function aim()
    local best, by = nil, -1e9
    for _, e in ipairs(Entity.find_by_tag("fruit")) do
        local x, y = Transform.get_position(e)
        if y ~= nil and y > by and y > -260 then best, by = e, y end
    end
    if best == nil then return end
    local x, y = Transform.get_position(best)
    local a = math.random() * math.pi
    local dx, dy = math.cos(a) * 160, math.sin(a) * 160
    ghost = { from = { x - dx, y - dy }, to = { x + dx, y + dy }, k = 0 }
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    toss_in = toss_in - dt
    if toss_in <= 0 then
        toss()
        if math.random() < 0.3 then toss() end
        toss_in = 0.55 + math.random() * 0.5
    end
    -- Where the blade is: the pointer, or the ghost after three still seconds.
    local mx, my = Input.get_mouse_world_position()
    if mouse_x ~= nil and (math.abs(mx - mouse_x) > 1 or math.abs(my - mouse_y) > 1) then idle = 0 else idle = idle + dt end
    mouse_x, mouse_y = mx, my
    local bx, by = mx, my
    if idle > 3 then
        if ghost.k >= 1 then aim() end
        if ghost.from ~= nil then
            ghost.k = math.min(1, ghost.k + dt / 0.14)
            bx = ghost.from[1] + (ghost.to[1] - ghost.from[1]) * ghost.k
            by = ghost.from[2] + (ghost.to[2] - ghost.from[2]) * ghost.k
        end
    end
    Transform.set_position(blade, bx, by)
    -- Cut along the stretch the blade moved, if it moved fast enough to be a
    -- swipe; count the fruit that are no longer whole.
    if last_x ~= nil then
        local d = math.sqrt((bx - last_x) ^ 2 + (by - last_y) ^ 2)
        if d > 6 then
            Physics.slice(last_x, last_y, bx, by)
            local c = COLOURS[1 + math.floor(t * 7) % #COLOURS]
            Particles.set(juice, "color_start", c)
            Particles.set(juice, "color_end", { c[1], c[2], c[3], 0 })
        end
    end
    last_x, last_y = bx, by
    -- Fruit that fell back down whole are gone; those missing otherwise
    -- were cut.
    local alive = {}
    for _, e in ipairs(Entity.find_by_tag("fruit")) do alive[e] = true end
    for e, i in pairs(known) do
        if alive[e] then
            local x, y = Transform.get_position(e)
            if y ~= nil and y < -440 then
                Entity.destroy(e)
                known[e] = nil
                missed = missed + 1
            end
        elseif t > 0.2 then
            known[e] = nil
            cuts = cuts + 1
            Particles.emit_burst(juice, 24)
        end
    end
    local who = "you"
    if idle > 3 then who = "the ghost blade" end
    UI.set_text(status, cuts .. " cut, " .. missed .. " missed   (" .. who .. ")")
end
