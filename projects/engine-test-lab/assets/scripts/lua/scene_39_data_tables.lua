-- Scene 39 - Data Tables
--
-- assets/data/enemies.dataschema declares the typed columns; enemies.data holds
-- the rows. Nothing about the three cards below is authored in the scene beyond
-- blank placeholders - name, colour, size, stats and the loot they reference
-- all come from the tables at runtime.
--
-- All three runtimes now supply the tables: native walks `assets/`, the web
-- build gets a list from the builder, and the editor reads every `.data` file
-- and seeds the viewport. A nil here means the tables did not load, not that
-- the runtime lacks the feature.

script = { tick_rate = 10 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/39-data-tables.scene"

local ENEMIES = "data/enemies"
local LOOT = "data/loot"
local IDS = { "goblin", "skeleton", "golem" }

local readout

local function build()
    local ok = 0
    for i, id in ipairs(IDS) do
        local card = Entity.find_by_name("Card" .. i)
        local name_lbl = Entity.find_by_name("CardName" .. i)
        local stats_lbl = Entity.find_by_name("CardStats" .. i)
        local drop_lbl = Entity.find_by_name("CardDrop" .. i)

        local display = Data.get(ENEMIES, id, "display_name")
        local hp = Data.get(ENEMIES, id, "health")
        local speed = Data.get(ENEMIES, id, "speed")
        local family = Data.get(ENEMIES, id, "family")
        local elite = Data.get(ENEMIES, id, "elite")
        local tint = Data.get(ENEMIES, id, "tint")
        local size = Data.get(ENEMIES, id, "size")

        if display ~= nil then ok = ok + 1 end

        if name_lbl then
            Text.set(name_lbl, tostring(display or id))
        end
        if stats_lbl then
            Text.set(stats_lbl, string.format("hp %s   speed %s   %s%s",
                tostring(hp or "?"), tostring(speed or "?"),
                tostring(family or "?"),
                (elite == true or elite == 1) and "   ELITE" or ""))
        end
        if card then
            -- Colour and size are data, not scene values.
            if type(tint) == "table" then
                Sprite.set_color(card, tint[1] or 1, tint[2] or 1, tint[3] or 1, tint[4] or 1)
            end
            if type(size) == "table" then
                Transform.set_scale(card, (size[1] or 60) / 60, (size[2] or 60) / 60)
            end
        end
        if drop_lbl then
            -- A ref field arrives as { table = ..., id = ... }; follow it with
            -- a second get_data to read the row it points at.
            local drop = Data.get(ENEMIES, id, "drop")
            if type(drop) == "table" and drop.id then
                local loot_name = Data.get(LOOT, drop.id, "display_name")
                local gold = Data.get(LOOT, drop.id, "gold")
                Text.set(drop_lbl, string.format("drops %s (%s g)",
                    tostring(loot_name or drop.id), tostring(gold or "?")))
            else
                Text.set(drop_lbl, "drops -")
            end
        end
    end
    return ok
end

function init(entity)
    readout = Entity.find_by_name("Readout")
    local ok = build()
    if ok == 0 then
        log("[scene_39] get_data returned nil for every row - data tables are not loaded in this runtime")
    else
        log("[scene_39] ready - " .. ok .. " row(s) read from data/enemies.data")
    end
end

function update(entity, dt)
    local ok = build()
    if readout then
        if ok == 0 then
            Text.set(readout, "get_data returned nil - tables not loaded in this runtime (see BUGS.md)")
        else
            Text.set(readout, string.format("%d/%d rows read from data/enemies.data", ok, #IDS))
        end
    end
    nav.tick(SCENE_PATH)
end
