-- 122 Bullet Time: every few seconds the world slows to an eighth
-- (`Time.set_scale`) and the music bends down with it (`Audio.follow_time`),
-- while the hero keeps real time: his `time_scale` ignores the world, so his
-- trail and dust run on, and this script moves him by the real time it gets
-- back from the world's `dt` (`dt / Time.get_scale()`). The gunmen fire on
-- the world's time, so their bullets crawl - and in slow time each one
-- leaves ripples in the air behind it.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/122-bullet-time.scene"
local FLOOR = -220.0
local SLOW = 0.125
local HERO_SPEED, LEFT, RIGHT = 520, -330, 330
local BULLET_SPEED = 1700

local atan2 = math.atan2 or function(y, x) return math.atan(y, x) end
local hero, status
local guns = {}
local bullets, waves = {}, {}
local captions = true
local slow, manual, mode_t = false, false, 0
local scale = 1
local hx, dir, stride = -300, 1, 0

local function between(a, b) return a + math.random() * (b - a) end

function init(entity)
    hero = Entity.find_by_name("Hero")
    status = Entity.find_by_name("Status")
    for i = 0, 1 do
        local x, y = Transform.get_position(Entity.find_by_name("Flash" .. i))
        guns[#guns + 1] = { flash = Entity.find_by_name("Flash" .. i), x = x, y = y, wait = between(0.3, 0.9), lit = 0 }
    end
    Audio.follow_time(true)
    Audio.play_music("sounds/bt_music.wav", 0.7)
end

local function fire(g)
    local tx, ty = hx + between(-40, 40), FLOOR + between(50, 130)
    local dx, dy = tx - g.x, ty - g.y
    local len = math.sqrt(dx * dx + dy * dy)
    dx, dy = dx / len, dy / len
    local b = Entity.spawn_prefab("BtBullet")
    Transform.set_position(b, g.x, g.y)
    Transform.set_rotation(b, atan2(dy, dx))
    Physics.set_velocity(b, dx * BULLET_SPEED, dy * BULLET_SPEED)
    bullets[#bullets + 1] = { e = b, x = g.x, y = g.y, dx = dx, dy = dy, age = 0, run = 0 }
    g.lit = 1
    Particles.emit_burst(g.flash, 1)
    Audio.play_sound("sounds/bt_shot.wav", 0.5, between(0.92, 1.08))
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    -- The real time behind this frame: the world's dt, scaled back.
    local s = Time.get_scale()
    local real = s > 0 and dt / s or 0
    -- Three seconds at full speed, six of bullet time; Space takes over.
    mode_t = mode_t + real
    if Input.is_key_just_pressed("Space") then
        slow, manual, mode_t = not slow, true, 0
    elseif not manual and mode_t > (slow and 6 or 3) then
        slow, mode_t = not slow, 0
    end
    local want = slow and SLOW or 1
    scale = scale + (want - scale) * math.min(1, real * 5)
    Time.set_scale(scale)
    local k = (1 - scale) / (1 - SLOW)
    PostProcess.set_float("color_grade", "saturation", 1.05 - 0.6 * k)
    PostProcess.set_float("chromatic", "strength", 0.0015 + 0.006 * k)

    -- The hero runs on real time, back and forth between the crates.
    hx = hx + dir * HERO_SPEED * real
    if hx > RIGHT then hx, dir = RIGHT, -1 elseif hx < LEFT then hx, dir = LEFT, 1 end
    stride = stride + real * 13
    Transform.set_position(hero, hx, FLOOR + math.abs(math.sin(stride)) * 7)
    Transform.set_scale(hero, dir, 1)

    -- The gunmen keep the world's time.
    for _, g in ipairs(guns) do
        g.wait = g.wait - dt
        if g.wait <= 0 then
            fire(g)
            g.wait = between(0.35, 0.8)
        end
        g.lit = math.max(0, g.lit - dt * 10)
        Lighting.set_intensity(g.flash, g.lit * 3)
        Sprite.set_color(g.flash, 3.0, 2.4, 1.35, g.lit)
    end

    -- Bullets, by world time; in slow time each leaves ripples behind.
    for i = #bullets, 1, -1 do
        local b = bullets[i]
        b.age = b.age + dt
        local step = BULLET_SPEED * dt
        b.x, b.y = b.x + b.dx * step, b.y + b.dy * step
        b.run = b.run + step
        if slow and b.run > 170 then
            b.run = 0
            local w = Entity.spawn_prefab("BtWave")
            Transform.set_position(w, b.x, b.y)
            waves[#waves + 1] = { e = w, age = 0 }
        end
        if b.age > 1.15 then table.remove(bullets, i) end
    end
    for i = #waves, 1, -1 do
        local w = waves[i]
        w.age = w.age + dt
        Material.set_vec4(w.e, "progress", w.age / 0.6, 0, 0, 0)
        if w.age > 0.55 then table.remove(waves, i) end
    end
    UI.set_text(status, string.format("%s   time x%.2f", scale < 0.9 and "BULLET TIME" or "real time", scale))
end
