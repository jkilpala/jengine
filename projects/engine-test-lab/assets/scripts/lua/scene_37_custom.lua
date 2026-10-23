-- Scene 37 - Custom Components
--
-- Custom components are arbitrary `component -> { field = value }` bags stored
-- on an entity's CustomComponents map. There is no Rust struct, no entry in
-- core_registration.rs and no ComponentBundle field: the scene authors them as
-- plain JSON under `"custom"`, and scripts read and write them with
-- get_custom / set_custom. This is where script-only state belongs; a real ECS
-- component is only worth it when an engine system has to see the data.
--
-- Portability note: the host-JS adapter marshals numbers only, so booleans go
-- across as 0/1 and strings may not survive. Numeric fields are the portable
-- subset, which is why the readout below sticks to them.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/37-custom-components.scene"

local BOXES = { "Box1", "Box2", "Box3", "Box4" }
local START_HP = { 100, 100, 100, 100 }

local readout
local last = "-"

local function label_of(i)
    return Entity.find_by_name("BoxLabel" .. i)
end

local function refresh_labels()
    for i, name in ipairs(BOXES) do
        local e = Entity.find_by_name(name)
        local lbl = label_of(i)
        if e ~= nil and lbl ~= nil then
            local hp = Entity.get_custom(e, "Stats", "hp") or 0
            local lvl = Entity.get_custom(e, "Stats", "level") or 0
            local gold = Entity.get_custom(e, "Loot", "gold") or 0
            Text.set(lbl, string.format("hp %d  lvl %d  gold %d", hp, lvl, gold))
        end
    end
end

function init(entity)
    readout = Entity.find_by_name("Readout")
    refresh_labels()
    log("[scene_37] ready - Space damages a box, G grants gold, R resets")
end

function update(entity, dt)
    if Input.is_key_just_pressed("Space") then
        local i = math.random(1, #BOXES)
        local e = Entity.find_by_name(BOXES[i])
        if e ~= nil then
            local hp = Entity.get_custom(e, "Stats", "hp") or 0
            hp = math.max(0, hp - 15)
            -- The write is deferred like any other script command: it lands
            -- before the next frame's reads.
            Entity.set_custom(e, "Stats", "hp", hp)
            last = string.format("%s took 15 damage (hp %d)", BOXES[i], hp)
        end
    elseif Input.is_key_just_pressed("G") then
        for _, name in ipairs(BOXES) do
            local e = Entity.find_by_name(name)
            if e ~= nil then
                Entity.set_custom(e, "Loot", "gold", (Entity.get_custom(e, "Loot", "gold") or 0) + 10)
            end
        end
        last = "+10 gold to every box"
    elseif Input.is_key_just_pressed("R") then
        for i, name in ipairs(BOXES) do
            local e = Entity.find_by_name(name)
            if e ~= nil then
                Entity.set_custom(e, "Stats", "hp", START_HP[i])
                Entity.set_custom(e, "Loot", "gold", i * 25)
            end
        end
        last = "reset"
    end

    refresh_labels()
    if readout then
        Text.set(readout, "last: " .. last)
    end

    nav.tick(SCENE_PATH)
end
