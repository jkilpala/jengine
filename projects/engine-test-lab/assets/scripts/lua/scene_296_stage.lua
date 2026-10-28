-- Scene 296: a 3D stage (`stage`, `stage_quad`, `stage_billboard`). A
-- checkered floor and three boxes projected in perspective, posts and a
-- walker standing in it, a lamp following the walker. The camera circles on
-- its own (A), or Left / Right turn it and Up / Down tilt it with
-- `Stage.set`; Q / W bring it nearer or farther (more or less perspective).
-- D toggles depth of field, focused on the walker; Z the depth haze. Each
-- box throws one shadow from the lamp following the walker (`stage_caster`).
-- The red box is a `stage_group`: G turns it on its spot, walls, lid and
-- shadow as one.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/296-stage.scene"
local line, stage, walker
local auto, dof, haze = true, true, true
local t, yaw, pitch, distance = 0, 0, 0.6, 40
local red, red_yaw, turning = nil, 0, false

function init(entity)
    line = Entity.find_by_name("Line0")
    stage = Entity.find_by_name("Stage")
    walker = Entity.find_by_name("Walker")
    red = Entity.find_by_name("Red")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("A") then auto = not auto end
    if Input.is_key_just_pressed("D") then
        dof = not dof
        PostProcess.set_enabled("depth_of_field", dof)
    end
    if Input.is_key_just_pressed("Z") then
        haze = not haze
        PostProcess.set_enabled("depth_haze", haze)
    end
    if Input.is_key_just_pressed("G") then turning = not turning end
    if turning and red then
        red_yaw = red_yaw + dt * 0.8
        Stage.set(red, "yaw", red_yaw)
    end
    t = t + dt
    if auto then
        yaw = yaw + dt * 0.25
    else
        if Input.is_key_pressed("Left") then yaw = yaw - dt end
        if Input.is_key_pressed("Right") then yaw = yaw + dt end
    end
    if Input.is_key_pressed("Up") then pitch = math.min(1.3, pitch + dt * 0.5) end
    if Input.is_key_pressed("Down") then pitch = math.max(0.15, pitch - dt * 0.5) end
    if Input.is_key_pressed("Q") then distance = math.max(12, distance - dt * 15) end
    if Input.is_key_pressed("W") then distance = math.min(120, distance + dt * 15) end
    Stage.set(stage, "yaw", yaw)
    Stage.set(stage, "pitch", pitch)
    Stage.set(stage, "distance", distance)
    -- The walker strolls a circle round the middle of the floor.
    Stage.place(walker, math.cos(t * 0.4) * 4, math.sin(t * 0.4) * 4 - 2, 0)
    UI.set_text(line, "yaw " .. math.floor(yaw * 180 / math.pi % 360 + 0.5) .. "   pitch "
        .. math.floor(pitch * 180 / math.pi + 0.5) .. "   distance " .. math.floor(distance + 0.5)
        .. "   depth of field " .. (dof and "on" or "off") .. "   haze " .. (haze and "on" or "off")
        .. (auto and "   (circling)" or ""))
end
