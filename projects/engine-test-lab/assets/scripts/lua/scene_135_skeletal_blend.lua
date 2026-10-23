-- Scene 135: a skeletal player driven by script. 1 / 2 / 3 crossfade to
-- idle, walk and run (`Animation.crossfade`); Up / Down blend the run into
-- the walk (`Animation.set_blend`); I sets the paws' IK weight
-- (`Skeleton.set_ik_weight`). The paws' `ik_two_bone.ground` plants them
-- on the course's collider; this script only carries the body along it.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/135-skeletal-blend-and-foot-ik.scene"

local COURSE = { { -900, -200 }, { -350, -200 }, { -150, -110 }, { 40, -110 }, { 40, -80 }, { 200, -80 }, { 420, -200 }, { 900, -200 } }
local FLOOR = -104.0
local WALK_SPEED, RUN_SPEED = 154.84, 596.49
local PAWS = { "Cat_hn_paw", "Cat_hf_paw", "Cat_fn_paw", "Cat_ff_paw" }

local cat, status
local paws = {}
local gait, blend, ik = "idle", 0.0, true
local x, y, tilt, speed = -600.0, 0.0, 0.0, 0.0

local function ground(px)
    for i = 1, #COURSE - 1 do
        local a, b = COURSE[i], COURSE[i + 1]
        if px >= a[1] and px <= b[1] then
            if b[1] == a[1] then return math.max(a[2], b[2]) end
            return a[2] + (b[2] - a[2]) * (px - a[1]) / (b[1] - a[1])
        end
    end
    return COURSE[#COURSE][2]
end

local function show()
    if status then
        UI.set_prop(status, "text", gait .. (gait ~= "idle" and ("   run blend " .. math.floor(blend * 100 + 0.5) .. "%") or "")
            .. "   foot IK " .. (ik and "on" or "off"))
    end
end

local function go(name, clip, b)
    gait = name
    Animation.crossfade(cat, clip, 0.4)
    blend = b
    Animation.set_blend(cat, name == "idle" and "" or "cr_run", b)
    show()
end

function init(entity)
    cat = Entity.find_by_name("Cat")
    status = Entity.find_by_name("Status")
    for _, n in ipairs(PAWS) do
        local p = Entity.find_by_name(n)
        if p then paws[#paws + 1] = p end
    end
    if cat then x, y = Transform.get_position(cat) end
    -- Walking from the start, so the scene shows something before a key.
    if cat then go("walk", "cr_walk", 0.0) end
    show()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if not cat then return end
    if Input.is_key_just_pressed("1") then go("idle", "cr_idle", 0.0) end
    if Input.is_key_just_pressed("2") then go("walk", "cr_walk", 0.0) end
    if Input.is_key_just_pressed("3") then go("run", "cr_walk", 1.0) end
    if gait ~= "idle" then
        local was = blend
        if Input.is_key_pressed("Up") then blend = math.min(1.0, blend + dt * 0.5) end
        if Input.is_key_pressed("Down") then blend = math.max(0.0, blend - dt * 0.5) end
        if blend ~= was then Animation.set_blend(cat, "cr_run", blend); show() end
    end
    if Input.is_key_just_pressed("I") then
        ik = not ik
        for _, p in ipairs(paws) do Skeleton.set_ik_weight(p, ik and 1 or 0) end
        show()
    end
    local want = gait == "idle" and 0.0 or (WALK_SPEED + (RUN_SPEED - WALK_SPEED) * blend * blend)
    speed = speed + (want - speed) * math.min(1.0, dt * 3.0)
    x = x + speed * dt
    if x > 750 then x = -750 end
    local gh, gf = ground(x + 8), ground(x + 136)
    y = y + ((gh + gf) * 0.5 - FLOOR - y) * math.min(1.0, dt * 10.0)
    tilt = tilt + (math.atan((gf - gh) / 128) * 0.85 - tilt) * math.min(1.0, dt * 8.0)
    Transform.set_position(cat, x, y)
    Transform.set_rotation(cat, tilt)
end
