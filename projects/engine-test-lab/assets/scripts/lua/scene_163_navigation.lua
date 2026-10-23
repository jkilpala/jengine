-- Scene 163: the paths and the walking are the engine's (`nav_region` on the
-- floor, `nav_agent` on each disc); this asks Nav.find_path for the dotted
-- line, sends the agents where you click and slides the gate.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/163-navigation.scene"
local DOTS = 40
local agents, dots = {}, {}
local probe, gate, target, readout
local gate_shut = false
local px, py = 0, 0

function init(entity)
    for i = 1, 6 do agents[i] = Entity.find_by_name("Agent" .. i) end
    for i = 1, DOTS do dots[i] = Entity.find_by_name("Dot" .. i) end
    probe = Entity.find_by_name("Probe")
    gate = Entity.find_by_name("Gate")
    target = Entity.find_by_name("Target")
    readout = Entity.find_by_name("Readout")
    px, py = Transform.get_position(probe)
end

-- Spread the dots evenly along the path, however many corners it has.
local function draw_path(path)
    local total = 0
    for i = 2, #path do
        total = total + math.sqrt((path[i][1] - path[i - 1][1]) ^ 2 + (path[i][2] - path[i - 1][2]) ^ 2)
    end
    local leg, walked = 2, 0
    for k = 1, DOTS do
        local want = total * (k - 1) / (DOTS - 1)
        while leg < #path do
            local l = math.sqrt((path[leg][1] - path[leg - 1][1]) ^ 2 + (path[leg][2] - path[leg - 1][2]) ^ 2)
            if walked + l >= want then break end
            walked = walked + l
            leg = leg + 1
        end
        local a, b = path[leg - 1], path[leg]
        local l = math.sqrt((b[1] - a[1]) ^ 2 + (b[2] - a[2]) ^ 2)
        local t = 0
        if l > 0 then t = math.min(1, (want - walked) / l) end
        if dots[k] then Transform.set_position(dots[k], a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t) end
    end
    return total
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local mx, my = Input.get_mouse_world_position()
    local path = Nav.find_path(px, py, mx, my)
    if path then
        local total = draw_path(path)
        UI.set_text(readout, "Nav.find_path: " .. (#path - 2) .. " corners, " .. math.floor(total + 0.5) .. " long")
    else
        for k = 1, DOTS do if dots[k] then Transform.set_position(dots[k], 0, -2000) end end
        UI.set_text(readout, "Nav.find_path: no way there")
    end
    if Input.is_mouse_left_just_pressed() then
        local cx, cy = Nav.closest_point(mx, my)
        if cx then
            Transform.set_position(target, cx, cy)
            for _, a in ipairs(agents) do Nav.set_target(a, mx, my) end
        end
    end
    if Input.is_key_just_pressed("G") and gate then
        gate_shut = not gate_shut
        local gx = Transform.get_position(gate)
        Transform.set_position(gate, gx, gate_shut and 155.0 or 880.0)
    end
end
