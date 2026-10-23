-- 22 Menus & HUD: three `.gui` screens and the moves between them. Every
-- screen's outermost node is "card"; a change of screen fades and shrinks
-- the old card away and fades the new one in from a little larger - one
-- `opacity` and one `scale`, which everything on the card inherits. Buttons
-- ease into their hover by themselves (`hover_scale`, `hover_color`).
-- Click, or wait: after a few quiet seconds the demo walks the screens.

script = { tick_rate = 60 }

local nav = require("nav")
local M = require("mh_items")
local SCENE = "scenes/22-menus-and-hud.scene"

local MENU, INV, HUD = "ui/mh_menu.gui", "ui/mh_inventory.gui", "ui/mh_hud.gui"
local DOCS = { menu = MENU, inventory = INV, hud = HUD }
local T_OUT, T_IN = 0.22, 0.32
local EMBER = { 1.0, 0.345, 0.051, 1.0 }
local CLEAR = { 0, 0, 0, 0 }

local screen, pending = "menu", nil
local phase, pt = "in", 0.0
local quiet = 99.0                -- seconds since the player did anything; the demo drives past 8
local demo_t = 0.0

-- Inventory.
local selected = 0
local stats = { atk = 0.0, def = 0.0, wgt = 0.0 }
local target = { atk = 0.0, def = 0.0, wgt = 0.0 }

-- HUD.
local hp, hp_to, mp, mp_to, xp = 1.0, 1.0, 1.0, 1.0, 0.35
local gold, gold_shown, pop = 1240, 1240, 0.0
local cool = { 0, 0, 0, 0 }
local cool_len = { 4, 6, 3, 8 }
local hud_t, next_hit, next_cast, next_coin, next_toast = 0.0, 1.5, 2.2, 3.0, 1.0
local toast_t = -1.0
local rng = 22222

local function rand()
    rng = (rng * 16807) % 2147483647
    return rng / 2147483647
end

local function ease_out(k) return 1 - (1 - k) * (1 - k) * (1 - k) end

local function thousands(n)
    local s = tostring(math.floor(n + 0.5))
    local out = s:reverse():gsub("(%d%d%d)", "%1 "):reverse()
    return (out:gsub("^ ", ""))
end

local function card(doc, opacity, scale)
    GUI.set(doc, "card", "opacity", opacity)
    GUI.set(doc, "card", "scale", scale)
end

local function go(name)
    if name == screen or phase == "out" then return end
    pending, phase, pt = name, "out", 0.0
end

local function select_slot(i)
    GUI.set(INV, "slot_" .. selected, "border_color", CLEAR)
    selected = i
    GUI.set(INV, "slot_" .. i, "border_color", EMBER)
    local item = M.ITEMS[i + 1]
    if item then
        local u0 = item.icon / M.N_ICONS
        GUI.set(INV, "item_icon", "uv_rect", { u0, 0, u0 + 1 / M.N_ICONS, 1 })
        GUI.set(INV, "item_icon", "tint", { 1, 1, 1, 1 })
        GUI.set(INV, "item_name", "text", item.name)
        GUI.set(INV, "item_desc", "text", item.desc)
        target.atk, target.def, target.wgt = item.atk, item.def, item.wgt
    else
        GUI.set(INV, "item_icon", "tint", { 1, 1, 1, 0 })
        GUI.set(INV, "item_name", "text", "Empty")
        GUI.set(INV, "item_desc", "text", "Nothing here yet.")
        target.atk, target.def, target.wgt = 0, 0, 0
    end
end

local function cast(i)
    if cool[i] > 0 or mp_to < 0.12 then return end
    cool[i] = cool_len[i]
    mp_to = math.max(0, mp_to - 0.12 - 0.05 * i)
end

function init(entity)
    for _, b in ipairs({ "btn_play", "btn_inventory", "btn_settings", "btn_quit" }) do
        GUI.on_click(entity, MENU, b, "on_gui_click")
    end
    for _, b in ipairs({ "btn_equip", "btn_back" }) do GUI.on_click(entity, INV, b, "on_gui_click") end
    for i = 0, M.SLOTS - 1 do GUI.on_click(entity, INV, "slot_" .. i, "on_gui_click") end
    GUI.on_click(entity, HUD, "btn_menu", "on_gui_click")
    card(MENU, 0, 1.05)
    select_slot(0)
end

function on_gui_click(entity, node)
    quiet = 0
    if node == "btn_play" then go("hud")
    elseif node == "btn_inventory" then go("inventory")
    elseif node == "btn_settings" then GUI.set(MENU, "hint", "text", "Settings live in Project Settings")
    elseif node == "btn_quit" then GUI.set(MENU, "hint", "text", "H returns to the hub")
    elseif node == "btn_back" or node == "btn_menu" then go("menu")
    elseif node == "btn_equip" then
        local item = M.ITEMS[selected + 1]
        GUI.set(INV, "item_desc", "text", item and ("Equipped: " .. item.name) or "Nothing to equip.")
    else
        local i = tonumber(string.match(node, "^slot_(%d+)$"))
        if i then select_slot(i) end
    end
end

-- The demo: menu, inventory browsing, a stretch of play, and round again.
local DEMO = { { "menu", 4.0 }, { "inventory", 7.0 }, { "hud", 9.0 } }
local function demo(dt)
    demo_t = demo_t + dt
    local total = 0
    for _, s in ipairs(DEMO) do total = total + s[2] end
    local t = demo_t % total
    for _, s in ipairs(DEMO) do
        if t < s[2] then
            go(s[1])
            if s[1] == "inventory" then
                local want = math.floor(t / 1.1) % #M.ITEMS
                if want ~= selected then select_slot(want) end
            end
            return
        end
        t = t - s[2]
    end
end

local function tick_transition(dt)
    pt = pt + dt
    if phase == "out" then
        local k = math.min(1, pt / T_OUT)
        card(DOCS[screen], 1 - k, 1 - 0.05 * k)
        if k >= 1 then
            GUI.hide(DOCS[screen])
            screen = pending
            GUI.show(DOCS[screen])
            card(DOCS[screen], 0, 1.05)
            phase, pt = "in", 0.0
        end
    elseif phase == "in" then
        local k = math.min(1, pt / T_IN)
        local e = ease_out(k)
        card(DOCS[screen], e, 1.05 - 0.05 * e)
        if k >= 1 then phase = "idle" end
    end
end

local function tick_inventory(dt)
    local k = math.min(1, dt * 8)
    for key, v in pairs(stats) do
        local nv = v + (target[key] - v) * k
        if math.abs(nv - v) > 0.0005 then
            stats[key] = nv
            GUI.set(INV, "bar_" .. key, "value", nv)
        end
    end
end

local function tick_hud(dt)
    hud_t = hud_t + dt
    -- Knocks and heals.
    if hud_t > next_hit then
        next_hit = hud_t + 1.6 + rand() * 1.8
        if rand() < 0.65 then hp_to = math.max(0.15, hp_to - 0.08 - rand() * 0.18)
        else hp_to = math.min(1, hp_to + 0.25) end
    end
    -- The demo casts; keys 1-4 cast too.
    if hud_t > next_cast then
        next_cast = hud_t + 1.4 + rand() * 1.6
        cast(1 + math.floor(rand() * 4))
    end
    for i = 1, 4 do
        if Input.is_key_just_pressed(tostring(i)) then cast(i); quiet = 0 end
    end
    mp_to = math.min(1, mp_to + dt * 0.05)
    -- Coins: the counter runs up and the panel pops.
    if hud_t > next_coin then
        next_coin = hud_t + 2.5 + rand() * 2.5
        gold = gold + 5 + math.floor(rand() * 40)
        pop = 1.0
        xp = (xp + 0.07) % 1.0
        GUI.set(HUD, "xp", "value", xp)
    end
    if gold_shown < gold then
        gold_shown = math.min(gold, gold_shown + dt * 60)
        GUI.set(HUD, "gold", "text", thousands(gold_shown))
        GUI.set(INV, "gold_text", "text", thousands(gold_shown))
    end
    if pop > 0 then
        pop = math.max(0, pop - dt * 3)
        GUI.set(HUD, "gold_panel", "scale", 1 + 0.16 * ease_out(pop))
    end
    -- Bars ease to where they are going.
    local k = math.min(1, dt * 4)
    hp = hp + (hp_to - hp) * k
    mp = mp + (mp_to - mp) * k
    GUI.set(HUD, "hp", "value", hp)
    GUI.set(HUD, "mp", "value", mp)
    -- Cooldowns drain from the top.
    for i = 1, 4 do
        if cool[i] > 0 then
            cool[i] = math.max(0, cool[i] - dt)
            GUI.set(HUD, "cool_" .. (i - 1), "height", 72 * cool[i] / cool_len[i])
        end
    end
    -- The toast slides down, waits, and goes.
    if hud_t > next_toast and toast_t < 0 then
        toast_t = 0
        next_toast = hud_t + 9
    end
    if toast_t >= 0 then
        toast_t = toast_t + dt
        local a
        if toast_t < 0.35 then a = ease_out(toast_t / 0.35)
        elseif toast_t < 2.8 then a = 1
        elseif toast_t < 3.2 then a = 1 - (toast_t - 2.8) / 0.4
        else a = 0; toast_t = -1 end
        GUI.set(HUD, "toast", "opacity", a)
        GUI.set(HUD, "toast", "offset_y", -40 * (1 - a))
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_mouse_left_just_pressed() then quiet = 0 end
    if Input.is_key_just_pressed("Escape") and screen ~= "menu" then go("menu"); quiet = 0 end
    quiet = quiet + dt
    if quiet > 8 then demo(dt) end
    tick_transition(dt)
    if screen == "inventory" then tick_inventory(dt) end
    if screen == "hud" then tick_hud(dt) end
end
