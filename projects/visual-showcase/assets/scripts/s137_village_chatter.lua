-- 137 Village Chatter: three villagers, each a `dialogue` playing its own
-- data table (`dialogue/*.data`) into one box of entity UI, sharing the
-- `Village` entity's `DialogueVars`. Click a villager to talk; Space
-- advances (finishing a line still typing first); 1, 2, 3 choose. Left
-- alone, a visitor works the square out by themselves - riddle, bread, gate.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/137-village-chatter.scene"
local PEOPLE = { "Bess", "Wren", "Holt" }
local START = { gold = 2, loaves = 0, met_baker = false, riddle_done = false, gate_open = false, grumpy = false }
local FEET = -165.0
local village, gate, panel, status
local talking = nil
local captions = true
local idle, wait = 6, 0
local gate_shown = false
-- The visitor's plan: who to talk to, and the choice to take at each fork.
local PLAN = {
    { "Holt", { 1 } },
    { "Bess", { 1 } },
    { "Wren", { 1, 1, 1, 2 } },
    { "Bess", { 1 } },
    { "Holt", { 1 } },
}
local step, picks = 1, {}

local function num(v) return v and tonumber(v) or 0 end

local function talk(name)
    if talking ~= nil and talking ~= name then Dialogue.stop(Entity.find_by_name(talking)) end
    talking = name
    Dialogue.start(Entity.find_by_name(name))
end

local function reset()
    for k, v in pairs(START) do Entity.set_custom(village, "DialogueVars", k, v) end
    step = 1
    picks = {}
end

function init(entity)
    village = Entity.find_by_name("Village")
    gate = Entity.find_by_name("Gate")
    panel = Entity.find_by_name("Panel")
    status = Entity.find_by_name("Status")
end

local function playing()
    if talking == nil then return false end
    return Entity.get_custom(Entity.find_by_name(talking), "Dialogue", "playing") == true
end

local function pressed_villager()
    if not Input.is_mouse_left_just_pressed() then return nil end
    local mx, my = Input.get_mouse_world_position()
    for _, name in ipairs(PEOPLE) do
        local x = Transform.get_position(Entity.find_by_name(name))
        if math.abs(mx - x) < 80 and my > FEET and my < FEET + 290 then return name end
    end
    return "elsewhere"
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    local who = pressed_villager()
    local acted = who ~= nil
    if who ~= nil and who ~= "elsewhere" then
        talk(who)
    elseif who == "elsewhere" or Input.is_key_just_pressed("Space") then
        if playing() then Dialogue.advance(Entity.find_by_name(talking)) end
        acted = true
    end
    for n = 1, 3 do
        if Input.is_key_just_pressed(tostring(n)) and playing() then
            Dialogue.choose(Entity.find_by_name(talking), n)
            acted = true
        end
    end
    if acted then idle = 0 else idle = idle + dt end

    if idle > 6 then
        -- The visitor: read each line through, then go on or choose.
        wait = wait - dt
        if not playing() then
            if wait <= 0 then
                if step > #PLAN then
                    reset()
                    wait = 2
                else
                    talk(PLAN[step][1])
                    picks = {}
                    for i, v in ipairs(PLAN[step][2]) do picks[i] = v end
                    step = step + 1
                    wait = 1.2
                end
            end
        else
            local e = Entity.find_by_name(talking)
            if Entity.get_custom(e, "Dialogue", "typing") == true then
                wait = 1.4
            elseif wait <= 0 then
                if num(Entity.get_custom(e, "Dialogue", "choices")) > 0 then
                    Dialogue.choose(e, table.remove(picks, 1) or 1)
                else
                    Dialogue.advance(e)
                end
                wait = 0.4
            end
        end
    end

    -- The box shows only while someone talks; the gate follows the vars.
    UI.set_visible(panel, playing())
    local open = Entity.get_custom(village, "DialogueVars", "gate_open") == true
    if open ~= gate_shown then
        Sprite.set_color(gate, 1, 1, 1, open and 0 or 1)
        gate_shown = open
    end
    UI.set_text(status, "coins " .. math.floor(num(Entity.get_custom(village, "DialogueVars", "gold")) + 0.5)
        .. "    loaves " .. math.floor(num(Entity.get_custom(village, "DialogueVars", "loaves")) + 0.5)
        .. "    gate " .. (open and "open" or "shut"))
end
