-- 143 Card Table: every card is a sprite with a `sprite_tilt`. The script
-- only says where each card should lean - `SpriteTilt.set(card, "target",
-- { yaw, pitch })` - and the engine eases it there and draws it turned in
-- perspective, its back (`back_texture`) showing once it is over. Left
-- alone for a few seconds, a ghost pointer plays the hand by itself.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/143-card-table.scene"
local HAND = { { "Ace", -300.0, -162.0, 0.1400, "the Ace of Spades" }, { "King", -150.0, -138.0, 0.0700, "the King of Hearts" }, { "Queen", 0.0, -130.0, -0.0000, "the Queen of Diamonds" }, { "Jack", 150.0, -138.0, -0.0700, "the Jack of Clubs" }, { "Ten", 300.0, -162.0, -0.1400, "the Ten of Hearts" } }
local HALF_W, HALF_H = 80.0, 112.0
local LEAN = 0.5
local status
local captions = true
local cards = {}
local river = {}
local coin
local t = 0
local last_mx, last_my
local idle = 99
local auto_flip = 0
local river_clock, river_next = 0.6, 1

local function clamp(v, lo, hi)
    if v < lo then return lo end
    if v > hi then return hi end
    return v
end

function init(entity)
    status = Entity.find_by_name("Status")
    for i, c in ipairs(HAND) do
        cards[i] = { e = Entity.find_by_name(c[1]), shadow = Entity.find_by_name(c[1] .. "Shadow"),
                     x = c[2], y = c[3], turn = c[4], label = c[5], flipped = false, lift = 0 }
    end
    for i = 1, 3 do
        river[i] = { e = Entity.find_by_name("River" .. i), up = false }
    end
    coin = Entity.find_by_name("Coin")
end

-- The pointer, or - after three still seconds - a ghost one sweeping the hand.
local function pointer(dt)
    local mx, my = Input.get_mouse_world_position()
    if last_mx ~= nil and (math.abs(mx - last_mx) > 0.5 or math.abs(my - last_my) > 0.5) then
        idle = 0
    else
        idle = idle + dt
    end
    last_mx, last_my = mx, my
    if idle < 3 then return mx, my, false end
    return math.sin(t * 0.5) * 360, -140 + math.sin(t * 1.9) * 60, true
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    local px, py, ghost = pointer(dt)
    -- The card under the pointer: the last one, which is drawn on top.
    local over
    for i, c in ipairs(cards) do
        if math.abs(px - c.x) < HALF_W and math.abs(py - c.y - c.lift) < HALF_H then over = i end
    end
    local click = (not ghost) and Input.is_mouse_left_just_pressed()
    if ghost then
        auto_flip = auto_flip + dt
        if auto_flip > 2.4 and over ~= nil then
            auto_flip = 0
            click = true
        end
    end
    if click and over ~= nil then cards[over].flipped = not cards[over].flipped end
    for i, c in ipairs(cards) do
        local base, yaw, pitch, lift = 0, 0, 0, 0
        if c.flipped then base = math.pi end
        if i == over then
            -- The side under the pointer sinks away, as a card pressed there
            -- would; turned over, its left and right have swapped.
            local side = 1
            if c.flipped then side = -1 end
            yaw = side * clamp((px - c.x) / HALF_W, -1, 1) * LEAN
            pitch = clamp((py - c.y - c.lift) / HALF_H, -1, 1) * LEAN
            lift = 40
        end
        c.lift = c.lift + (lift - c.lift) * math.min(1, dt * 10)
        local k = c.lift / 40
        Transform.set_position(c.e, c.x, c.y + c.lift)
        Transform.set_rotation(c.e, c.turn * (1 - k))
        Transform.set_scale(c.e, 1 + 0.08 * k, 1 + 0.08 * k)
        Transform.set_position(c.shadow, c.x + 10 + 14 * k, c.y - 14 + c.lift * 0.55)
        Transform.set_rotation(c.shadow, c.turn * (1 - k))
        SpriteTilt.set(c.e, "target", { base + yaw, pitch })
    end
    -- The table's cards turn over one at a time.
    river_clock = river_clock + dt
    if river_clock > 1.5 then
        river_clock = 0
        local r = river[river_next]
        r.up = not r.up
        local yaw = math.pi
        if r.up then yaw = 0 end
        SpriteTilt.set(r.e, "target", { yaw, 0 })
        river_next = river_next % #river + 1
    end
    -- The coin spins on its axis, crown then numeral.
    SpriteTilt.set(coin, "yaw", t * 2.4)
    if over ~= nil then
        local c = cards[over]
        local how = "leans toward the pointer"
        if c.flipped then how = "is face down, leaning toward the pointer" end
        local who = "you"
        if ghost then who = "the ghost hand" end
        UI.set_text(status, c.label .. " " .. how .. " (" .. who .. ")")
    else
        UI.set_text(status, "point at a card")
    end
end
