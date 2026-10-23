-- Scene 166: the following is the engine's (`camera_follow` on the disc, its
-- dead zone and look-ahead switched by Camera.set_follow); this moves the
-- disc and draws the dead zone where Camera.get_position says the camera is.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/166-camera-follow.scene"
local MODES = {
    { name = "follow", zone = { 0, 0 }, ahead = 0 },
    { name = "dead zone", zone = { 320, 200 }, ahead = 0 },
    { name = "look-ahead", zone = { 0, 0 }, ahead = 0.5 },
    { name = "dead zone + look-ahead", zone = { 320, 200 }, ahead = 0.5 },
}
local runner, readout, centre
local bars = {}
local mode = 4
local x, y = 0, -80

local function apply(m)
    mode = m
    Camera.set_follow(runner, "dead_zone", MODES[m].zone)
    Camera.set_follow(runner, "look_ahead", MODES[m].ahead)
end

function init(entity)
    runner = Entity.find_by_name("Runner")
    readout = Entity.find_by_name("Readout")
    centre = Entity.find_by_name("Centre")
    for _, side in ipairs({ "Top", "Bottom", "Left", "Right" }) do bars[side] = Entity.find_by_name("Zone" .. side) end
    apply(4)
end

local function held(a, b) return Input.is_key_pressed(a) or Input.is_key_pressed(b) end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    for i = 1, 4 do
        if Input.is_key_just_pressed(tostring(i)) then apply(i) end
    end
    local speed = Input.is_key_pressed("ShiftLeft") and 600 or 260
    if held("A", "Left") then x = x - speed * dt end
    if held("D", "Right") then x = x + speed * dt end
    if held("W", "Up") then y = y + speed * dt end
    if held("S", "Down") then y = y - speed * dt end
    x = math.max(-500, math.min(3500, x))
    y = math.max(-300, math.min(300, y))
    Transform.set_position(runner, x, y)

    local cx, cy = Camera.get_position()
    local z = MODES[mode].zone
    local w, h = z[1], z[2]
    local show = w > 0
    Transform.set_position(bars.Top, cx, cy + h / 2)
    Transform.set_position(bars.Bottom, cx, cy - h / 2)
    Transform.set_position(bars.Left, cx - w / 2, cy)
    Transform.set_position(bars.Right, cx + w / 2, cy)
    Transform.set_scale(bars.Top, show and w or 0, 3)
    Transform.set_scale(bars.Bottom, show and w or 0, 3)
    Transform.set_scale(bars.Left, 3, show and h or 0)
    Transform.set_scale(bars.Right, 3, show and h or 0)
    Transform.set_position(centre, cx, cy)
    Text.set(readout, "mode: " .. MODES[mode].name .. "    Camera.get_position() = " .. math.floor(cx + 0.5) .. ", " .. math.floor(cy + 0.5)
        .. "    disc " .. math.floor(x - cx + 0.5) .. " from the centre")
end
