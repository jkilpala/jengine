-- Scene 38 - Save System
--
-- Three calls make up the whole API:
--   Save.set(slot, key, value)  buffers a write (value may be a nested table)
--   Save.commit(slot)           writes the slot to storage - nothing persists without it
--   Save.get(slot, key)         reads back; nil for an unknown slot AND an unknown key
--   Save.clear(slot)            drops the slot
--
-- The interesting part to verify across runtimes is that a value survives a
-- commit and a scene reload: leave the scene with H, come back, press L.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/38-save.scene"

local slot = 1
local counter = 0
local last = "-"

local lbl_slot, lbl_buffered, lbl_stored, lbl_nested, readout

local function refresh()
    if lbl_slot then
        Text.set(lbl_slot, "slot: " .. slot)
    end
    if lbl_buffered then
        Text.set(lbl_buffered, "counter (this session): " .. counter)
    end
    if lbl_stored then
        local v = Save.get(slot, "counter")
        Text.set(lbl_stored, "save_get(" .. slot .. ", \"counter\") = " .. tostring(v))
    end
    if lbl_nested then
        local inv = Save.get(slot, "inventory")
        if type(inv) == "table" then
            -- Nested tables round-trip, so a whole block of state is one key.
            Text.set(lbl_nested, string.format(
                "inventory = { gold = %s, potions = %s }",
                tostring(inv.gold), tostring(inv.potions)))
        else
            Text.set(lbl_nested, "inventory = " .. tostring(inv))
        end
    end
    if readout then
        Text.set(readout, "last: " .. last)
    end
end

function init(entity)
    lbl_slot = Entity.find_by_name("SlotLabel")
    lbl_buffered = Entity.find_by_name("Buffered")
    lbl_stored = Entity.find_by_name("Stored")
    lbl_nested = Entity.find_by_name("Nested")
    readout = Entity.find_by_name("Readout")
    refresh()
    log("[scene_38] ready - 1/2/3 slot, S set, C commit, L load, X clear")
end

function update(entity, dt)
    if Input.is_key_just_pressed("1") then
        slot = 1; last = "slot 1"
    elseif Input.is_key_just_pressed("2") then
        slot = 2; last = "slot 2"
    elseif Input.is_key_just_pressed("3") then
        slot = 3; last = "slot 3"
    elseif Input.is_key_just_pressed("S") then
        counter = counter + 1
        Save.set(slot, "counter", counter)
        Save.set(slot, "inventory", { gold = counter * 10, potions = counter })
        last = "save_set (buffered - not on disk until C)"
    elseif Input.is_key_just_pressed("C") then
        Save.commit(slot)
        last = "save_commit(" .. slot .. ")"
    elseif Input.is_key_just_pressed("L") then
        local v = Save.get(slot, "counter")
        counter = tonumber(v) or 0
        last = "loaded counter = " .. tostring(v)
    elseif Input.is_key_just_pressed("X") then
        Save.clear(slot)
        counter = 0
        last = "save_clear(" .. slot .. ")"
    end

    refresh()
    nav.tick(SCENE_PATH)
end
