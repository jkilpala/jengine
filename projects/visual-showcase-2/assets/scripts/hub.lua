-- The hub: the scene list as rows of entity-UI text over a little plaza at
-- night, chosen with Up / Down (or a pad) and entered with Enter. The
-- traveller stands in the middle and the camera turns slowly round them,
-- so the hub is itself a stage.

local nav = require("nav")

local ROWS = 6
local IDLE = { 0.6, 0.62, 0.72, 1.0 }
local CHOSEN = { 1.0, 0.86, 0.55, 1.0 }

local rows = {}
local blurb, stage
local chosen = 1
local yaw = 0.0

-- Break a paragraph into lines of at most `width` characters: UI text does
-- not wrap on its own.
local function wrap(s, width)
    local lines, line = {}, ""
    for word in string.gmatch(s, "%S+") do
        if #line > 0 and #line + 1 + #word > width then
            lines[#lines + 1] = line
            line = word
        else
            line = (#line > 0) and (line .. " " .. word) or word
        end
    end
    if #line > 0 then lines[#lines + 1] = line end
    return table.concat(lines, "\n")
end

local function paint()
    for i = 1, ROWS do
        local s = nav.SCENES[i]
        if rows[i] then
            UI.set_text(rows[i], s and ((i == chosen and "> " or "   ") .. s.title) or "")
            local c = i == chosen and CHOSEN or IDLE
            UI.set_prop(rows[i], "color", c)
        end
    end
    if blurb and nav.SCENES[chosen] then UI.set_text(blurb, wrap(nav.SCENES[chosen].blurb, 46)) end
end

function init(entity)
    for i = 1, ROWS do rows[i] = Entity.find_by_name("Row" .. i) end
    blurb = Entity.find_by_name("Blurb")
    stage = Entity.find_by_name("Stage")
    chosen = math.max(1, math.min(#nav.SCENES, nav.last_choice()))
    paint()
end

function update(entity, dt)
    yaw = yaw + dt * 0.08
    if stage then Stage.set(stage, "yaw", yaw) end
    local n = #nav.SCENES
    if Input.is_action_just_pressed("move_down") then
        chosen = chosen % n + 1
        paint()
    elseif Input.is_action_just_pressed("move_up") then
        chosen = (chosen - 2) % n + 1
        paint()
    end
    if Input.is_action_just_pressed("confirm") then
        nav.remember_choice(chosen)
        nav.go(nav.SCENES[chosen].path)
    elseif Input.is_action_just_pressed("next_scene") then
        nav.go(nav.SCENES[1].path)
    elseif Input.is_action_just_pressed("prev_scene") then
        nav.go(nav.SCENES[n].path)
    end
end
