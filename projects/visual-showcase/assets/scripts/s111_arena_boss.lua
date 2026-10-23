-- 111 Arena Boss: a stone golem against a knight. The golem's body is the
-- engine's - a base clip, a breath layer, a flinch and a roar on layers, its
-- fist reaching by IK, a ragdoll when it falls; the fight is this script:
-- which attack, when, where. Every attack is told first by a shape on the
-- floor that fills (radial and bar sprite fills), and where it lands the
-- floor is carved. When its health bar is empty it goes limp, and after a
-- while the arena is set again.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/111-arena-boss.scene"
local FLOOR = -190.000000
local GX = -430.000000

local e = {}
local t = 0
local rng = 111
local captions = true
local hp = 1.0
local state = "idle"
local st = 0
local next_attack = 2.5
local attack_i = 0
local slam_x = 0
local rain = {}
local rocks = {}
local glow = 1.0
local hero = { x = 330.000000, y = FLOOR, vx = 0, jump = 0, cool = 1.0, swing = 0, goal = 0 }
local dead_for = 0

local function rand()
    rng = (rng * 16807) % 2147483647
    return rng / 2147483647
end
local function between(a, b) return a + (b - a) * rand() end
local function spawn(name, x, y)
    local h = Entity.spawn_prefab(name)
    Transform.set_position(h, x, y)
    return h
end
local function park(id) Transform.set_position(id, 0, -5000) end

local function impact(x, size)
    Sprite.carve(e.Floor, x, FLOOR, size)
    spawn("AbDust", x, FLOOR + 10)
    spawn("AbDebris", x, FLOOR + 14)
    spawn("AbFlash", x, FLOOR + 40)
    Camera.shake(size * 0.12, 0.35, 22)
end

function init(entity)
    for _, n in ipairs({ "Golem", "GolemForeR", "GolemTorso", "GolemEyes", "Floor", "SlamTell", "SlamRing", "SweepTell",
                         "BossBar", "Hero", "HeroThighL", "HeroThighR", "HeroShinL", "HeroShinR", "HeroArmR", "HeroForeR",
                         "HeroArmL", "Status" }) do
        e[n] = Entity.find_by_name(n)
    end
    for i = 0, 2 do rain[i + 1] = { e = Entity.find_by_name("RainTell" .. i), x = 0 } end
    e.glows = {}
    for _, n in ipairs({ "Pelvis", "Torso", "Head", "UpperL", "ForeL", "UpperR", "ForeR", "ThighL", "ShinL", "ThighR", "ShinR" }) do
        e.glows[#e.glows + 1] = Entity.find_by_name("Golem" .. n .. "Glow")
    end
end

-- The hand's reach: where it is held up before a blow, and where it lands.
local function reach(x, y, w)
    Skeleton.set_ik_target(e.GolemForeR, x, y)
    Skeleton.set_ik_weight(e.GolemForeR, w)
end

local function hit_golem()
    hp = math.max(0, hp - 0.06)
    glow = 2.2
    spawn("AbSpark", GX + 120, FLOOR + 260 + between(-40, 60))
    Animation.play_layer(e.Golem, "flinch", "animations/gb_flinch.skelanim", 0.05, false)
    Sprite.set_fill(e.BossBar, hp)
end

local function pose_hero(dt)
    -- Walking legs while moving, a thrust of the spear when attacking, a
    -- hop over a sweep.
    local moving = math.abs(hero.vx) > 5
    local p = t * 9
    local swing = moving and 0.35 * math.sin(p) or 0
    Transform.set_rotation(e.HeroThighL, -math.pi + 0.1 + swing)
    Transform.set_rotation(e.HeroThighR, math.pi - 0.14 - swing)
    Transform.set_rotation(e.HeroShinL, 0.04 + (moving and math.max(0, -math.sin(p)) * 0.5 or 0))
    Transform.set_rotation(e.HeroShinR, -0.06 - (moving and math.max(0, math.sin(p)) * 0.5 or 0))
    local thrust = hero.swing > 0 and math.sin((1 - hero.swing / 0.35) * math.pi) or 0
    Transform.set_rotation(e.HeroArmR, 2.9 - 0.7 * thrust)
    Transform.set_rotation(e.HeroForeR, -1.6 + 1.3 * thrust)
    local hy = FLOOR + 98 + hero.jump
    Transform.set_position(e.Hero, hero.x, hy)
end

local function tell_disc(id, x, amount)
    Transform.set_position(id, x, FLOOR + 6)
    Sprite.set_fill(id, amount)
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    st = st + dt

    -- The cracks glow brighter when struck, and fade back.
    glow = glow + (1 - glow) * math.min(1, dt * 3)
    local k = (state == "dead") and math.max(0, 1 - dead_for / 2) or glow
    for _, g in ipairs(e.glows) do Sprite.set_color(g, 2.0 * k, 0.9 * k, 0.24 * k, 0.85) end
    Lighting.set_intensity(e.GolemEyes, 1.2 * k)
    Sprite.set_color(e.GolemEyes, 6 * k, 3.3 * k, 0.9 * k, 1)

    -- The knight: closes in and thrusts when nothing is coming, keeps clear
    -- of every warning, hops the sweep.
    hero.cool = hero.cool - dt
    hero.swing = math.max(0, hero.swing - dt)
    local goal = GX + 250
    if state == "slam_tell" or state == "slam" then goal = slam_x + 300 end
    if state == "rain" or state == "rain_tell" then
        goal = GX + 420
        for _, r in ipairs(rain) do
            if math.abs(goal - r.x) < 110 then goal = r.x + 150 end
        end
    end
    if state == "dead" then goal = GX + 360 end
    local dx = goal - hero.x
    hero.vx = math.max(-420, math.min(420, dx * 5))
    hero.x = hero.x + hero.vx * dt
    if state == "idle" and math.abs(dx) < 40 and hero.cool <= 0 then
        hero.swing = 0.35
        hero.cool = between(0.45, 0.7)
    end
    if hero.swing > 0 and hero.swing - dt <= 0.17 and hero.swing > 0.17 then hit_golem() end
    if state == "sweep" then
        hero.jump = math.max(0, math.sin(math.min(1, st / 0.7) * math.pi) * 150)
    else
        hero.jump = math.max(0, hero.jump - dt * 400)
    end
    pose_hero(dt)

    if state == "dead" then
        dead_for = dead_for + dt
        if dead_for > 7 then Scene.load(SCENE) end
        UI.set_text(e.Status, "the warden falls")
        return
    end
    if hp <= 0 then
        state = "dead"
        reach(GX + 200, FLOOR, 0)
        Ragdoll.set(e.Golem, "active", true)
        spawn("AbDust", GX, FLOOR + 40)
        Camera.shake(6, 0.6, 18)
        park(e.SlamTell); park(e.SlamRing); park(e.SweepTell)
        for _, r in ipairs(rain) do park(r.e) end
        return
    end

    if state == "idle" then
        reach(GX + 200, FLOOR + 100, 0)
        next_attack = next_attack - dt
        if next_attack <= 0 then
            attack_i = attack_i + 1
            local kind = ({ "slam", "sweep", "slam", "rain" })[(attack_i - 1) % 4 + 1]
            st = 0
            if kind == "slam" then
                slam_x = math.max(GX + 180, math.min(GX + 420, hero.x - 20))
                state = "slam_tell"
            elseif kind == "sweep" then
                state = "sweep_tell"
                Transform.set_position(e.SweepTell, GX + 80, FLOOR + 8)
            else
                state = "rain_tell"
                Animation.play_layer(e.Golem, "roar", "animations/gb_roar.skelanim", 0.2, false)
                for i, r in ipairs(rain) do r.x = GX + 260 + i * 170 + between(-40, 40) end
            end
        end
    elseif state == "slam_tell" then
        -- The fist rises over the spot while the warning fills.
        local p = math.min(1, st / 1.3)
        tell_disc(e.SlamTell, slam_x, p)
        Transform.set_position(e.SlamRing, slam_x, FLOOR + 6)
        reach(slam_x - 30, FLOOR + 280 + 60 * p, math.min(1, st * 3))
        if st >= 1.3 then state = "slam"; st = 0 end
    elseif state == "slam" then
        reach(slam_x, FLOOR - 10, 1)
        if st >= 0.12 and st - dt < 0.12 then
            impact(slam_x, 70)
            park(e.SlamTell)
            park(e.SlamRing)
        end
        if st >= 0.9 then state = "idle"; next_attack = between(2.6, 3.4) end
    elseif state == "sweep_tell" then
        local p = math.min(1, st / 1.4)
        Sprite.set_fill(e.SweepTell, p)
        reach(GX + 160, FLOOR + 160, math.min(1, st * 3))
        if st >= 1.4 then state = "sweep"; st = 0 end
    elseif state == "sweep" then
        -- The fist drags along the floor, cutting a trench.
        local p = math.min(1, st / 0.7)
        local x = GX + 170 + p * 560
        reach(x, FLOOR - 6, 1)
        if math.floor(st * 30) ~= math.floor((st - dt) * 30) then Sprite.carve(e.Floor, x, FLOOR, 26) end
        if st >= 0.7 then
            Sprite.set_fill(e.SweepTell, 0)
            park(e.SweepTell)
            spawn("AbDust", x, FLOOR + 10)
            state = "idle"; next_attack = between(2.6, 3.4)
        end
    elseif state == "rain_tell" then
        local p = math.min(1, st / 1.4)
        for _, r in ipairs(rain) do tell_disc(r.e, r.x, p) end
        if st >= 1.4 then
            for _, r in ipairs(rain) do
                rocks[#rocks + 1] = { e = spawn("AbRock", r.x, 560), x = r.x, done = false }
            end
            state = "rain"; st = 0
        end
    elseif state == "rain" then
        for _, rk in ipairs(rocks) do
            if not rk.done then
                local x, y = Transform.get_position(rk.e)
                if y and y <= FLOOR + 40 then
                    rk.done = true
                    impact(rk.x, 46)
                end
            end
        end
        if st >= 1.6 then
            for _, r in ipairs(rain) do park(r.e) end
            rocks = {}
            state = "idle"; next_attack = between(2.6, 3.4)
        end
    end
    UI.set_text(e.Status, string.format("warden %d%%", math.floor(hp * 100 + 0.5)))
end
