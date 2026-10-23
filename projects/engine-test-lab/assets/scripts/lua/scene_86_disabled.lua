-- Scene 86 - Disabled buttons
--
-- `GuiNodeType::Button` has always had a `disabled` flag. Until Sep 2026 it
-- serialized, loaded, showed up in the GUI editor - and nothing read it. A
-- disabled button drew like any other and fired its callback like any
-- other, so a shop could neither show nor enforce what the player could not
-- afford, and every game here hid the row instead. "You cannot afford this"
-- and "this does not exist" are different sentences.
--
-- Two things to check, in all three runtimes:
--   1. a dim row does not fire `on_gui_click` when you click it, and
--   2. the only thing this script does to a row is set `disabled` - the
--      refusal itself is the engine's, at the hit-test.
--
-- [F] forces every row on regardless of the wallet: proof that the same
-- click that did nothing a moment ago goes straight through once the flag
-- is cleared.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local GUI_DOC = "ui/scene_86_shop.gui"
local SCENE_PATH = "scenes/86-disabled.scene"

local ROWS = {
    { id = "btn_buy_0", name = "TORCH", price = 10 },
    { id = "btn_buy_1", name = "LANTERN", price = 50 },
    { id = "btn_buy_2", name = "BRAZIER", price = 200 },
    { id = "btn_buy_3", name = "BEACON", price = 1000 },
}
local RATE = 25.0

local coin = 0.0
local forced = false
local owned = { 0, 0, 0, 0 }
local refused = 0
local state = { false, false, false, false }   -- what the engine was last told

local function can_afford(row)
    return coin >= row.price
end

local function refresh()
    for i, row in ipairs(ROWS) do
        local off = not (forced or can_afford(row))
        -- Only write when it changes: a gui_set every frame for every row is
        -- a command queue full of nothing.
        if off ~= state[i] then
            GUI.set(GUI_DOC, row.id, "disabled", off)
            state[i] = off
        end
        GUI.set(GUI_DOC, row.id, "text", string.format("%s  -  %d   (own %d)", row.name, row.price, owned[i]))
    end
    GUI.set(GUI_DOC, "wallet", "text", string.format("%d COIN", math.floor(coin)))
    GUI.set(GUI_DOC, "rate", "text", string.format("+%d a second%s", RATE,
        forced and "   [F] FORCED ON - every row clickable" or ""))
    local total = owned[1] + owned[2] + owned[3] + owned[4]
    GUI.set(GUI_DOC, "owned", "text", string.format("bought %d   clicks the engine refused: %d", total, refused))
end

function init(entity)
    for _, row in ipairs(ROWS) do
        GUI.on_click(entity, GUI_DOC, row.id, "on_gui_click")
    end
    for i = 1, #ROWS do
        state[i] = true
    end
    refresh()
    log("[scene_86] ready - the rows switch on as the wallet fills")
end

function update(entity, dt)
    coin = coin + RATE * dt

    if Input.is_key_just_pressed("F") then
        forced = not forced
    end
    if Input.is_key_just_pressed("R") then
        coin = 0.0
    end
    -- The keyboard path deliberately ignores `disabled`: the flag is about
    -- the *click*, and a game is free to sell the same thing another way.
    for i = 1, #ROWS do
        if Input.is_key_just_pressed(tostring(i)) then
            local row = ROWS[i]
            if coin >= row.price then
                coin = coin - row.price
                owned[i] = owned[i] + 1
                GUI.set(GUI_DOC, "log", "text", "key " .. i .. " -> bought a " .. row.name)
            else
                GUI.set(GUI_DOC, "log", "text", "key " .. i .. " -> not enough for a " .. row.name)
            end
        end
    end

    refresh()
    nav.tick(SCENE_PATH)
end

function on_gui_click(self_entity, node_id)
    for i, row in ipairs(ROWS) do
        if node_id == row.id then
            -- If this fires for a row the wallet cannot cover, the engine let
            -- a disabled button through - which is the bug this scene exists
            -- to catch, so say so loudly rather than quietly refusing.
            if not forced and not can_afford(row) then
                refused = refused + 1
                GUI.set(GUI_DOC, "log", "text", "BUG: a disabled row fired (" .. row.name .. ")")
                log("[scene_86] a disabled button fired its callback")
                return
            end
            if can_afford(row) then
                coin = coin - row.price
                owned[i] = owned[i] + 1
                GUI.set(GUI_DOC, "log", "text", "clicked -> bought a " .. row.name)
            else
                -- Reachable only under [F]: forced on, still not affordable.
                GUI.set(GUI_DOC, "log", "text", "clicked -> " .. row.name .. ", but the wallet is short")
            end
            return
        end
    end
end
