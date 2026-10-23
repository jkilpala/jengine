-- 138 Loot Satchel: two bags and an equipment board of `ui_drop_slot`s,
-- the loot `ui_draggable` items. Drag an item: a ghost follows the pointer
-- and the slot under it says yes (green) or no (red). Potions, coins and
-- apples stack; anything else swaps; the kit takes only its own kind. With a
-- controller, arrows walk the slots and Enter picks up and puts down. Left
-- alone, the kit is sorted by `UI.drop` - the same rules, from a script.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/138-loot-satchel.scene"
local ITEMS = { "Sword", "RedPotion", "RedPotion2", "Coins", "Apple", "Scroll", "Axe", "Shield", "Helm", "Ring", "Gem", "Key", "BluePotion" }
local status
local captions = true
local idle, wait, step = 6, 0, 1
-- The tidy-up: each move goes through the drop rules, so some are refused.
local PLAN = {
    { "Sword", "WeaponSlot" }, { "Shield", "ShieldSlot" }, { "Helm", "HelmSlot" },
    { "Apple", "RingSlot" }, { "Ring", "RingSlot" }, { "RedPotion2", "A2" },
    { "Axe", "WeaponSlot" }, { "Coins", "B12" }, { "BluePotion", "A4" }, { "Gem", "A12" },
}
local SAID = {
    { "the sword", "goes in the weapon slot" }, { "the shield", "on the arm" }, { "the helm", "on the head" },
    { "an apple", "is no ring - refused" }, { "the ring", "on a finger" }, { "two potions", "stack onto three" },
    { "the axe", "swaps with the sword" }, { "the coins", "into the other bag" }, { "the blue potion", "moves over" },
    { "the gem", "to the corner" },
}

function init(entity)
    status = Entity.find_by_name("Status")
end

local function held()
    for _, name in ipairs(ITEMS) do
        if Entity.get_custom(Entity.find_by_name(name), "Drag", "held") == true then return name end
    end
    return nil
end

function update(entity, dt)
    local holding = held()
    -- B is cancel while an item is held, not the way back to the hub.
    if holding == nil then nav.tick(SCENE) end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if holding ~= nil or Input.is_mouse_left_pressed() or Input.is_key_just_pressed("Enter")
        or Input.is_key_just_pressed("Up") or Input.is_key_just_pressed("Down") then
        idle = 0
    else
        idle = idle + dt
    end
    if holding ~= nil then
        UI.set_text(status, "carrying " .. string.lower(holding))
    elseif idle > 6 then
        wait = wait - dt
        if wait <= 0 then
            if step > #PLAN then
                Scene.load(SCENE)
                return
            end
            UI.drop(Entity.find_by_name(PLAN[step][1]), PLAN[step][2])
            UI.set_text(status, SAID[step][1] .. " " .. SAID[step][2])
            step = step + 1
            wait = 1.3
        end
    else
        UI.set_text(status, "potions, coins and apples stack; the kit takes only its own kind")
    end
end
