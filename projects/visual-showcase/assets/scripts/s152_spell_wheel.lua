-- 152 Spell Wheel: the wheel is a `ui_radial_menu`. The engine lays the
-- six badges round it, grows and lights the one the pointer or stick points
-- at, and on a click writes the pick into the wheel's custom `RadialMenu`
-- fields and calls `on_radial_pick` on this, the wheel's own script. Left
-- alone, the script aims the
-- wheel itself (`Radial.set(wheel, "aim", degrees)`) and casts.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/152-spell-wheel.scene"
local COLOURS = { fire = { 1.00, 0.45, 0.15 }, ice = { 0.45, 0.80, 1.00 }, wind = { 0.60, 1.00, 0.75 }, earth = { 0.80, 0.60, 0.30 }, lightning = { 1.00, 0.90, 0.30 }, heal = { 1.00, 0.50, 0.80 } }
local status, wheel, spell, flash
local captions = true
local t, idle, flash_t, cast_in = 0, 99, 0, 1.2
local last_mx, last_my
local cast_name = ""
local aiming = false

function init(entity)
    status = Entity.find_by_name("Status")
    wheel = entity
    spell = Entity.find_by_name("Spell")
    flash = Entity.find_by_name("Flash")
end

local function cast(name)
    local c = COLOURS[string.lower(name)]
    if c == nil then return end
    Particles.set(spell, "color_start", { c[1], c[2], c[3], 1 })
    Particles.set(spell, "color_end", { c[1], c[2], c[3], 0 })
    Particles.emit_burst(spell, 90)
    flash_t = 0.35
    Sprite.set_color(flash, c[1], c[2], c[3], 0.9)
    cast_name = name
end

-- The wheel picked a section (a click, or A).
function on_radial_pick(entity)
    cast(Entity.get_custom(wheel, "RadialMenu", "label") or "")
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    local mx, my = Input.get_mouse_position()
    if last_mx ~= nil and (math.abs(mx - last_mx) > 1 or math.abs(my - last_my) > 1) then idle = 0 else idle = idle + dt end
    last_mx, last_my = mx, my
    -- Left alone: sweep the wheel round and cast what it points at.
    if idle > 3 then
        aiming = true
        Radial.set(wheel, "aim", 90 - t * 70)
        cast_in = cast_in - dt
        if cast_in <= 0 then
            local i = Entity.get_custom(wheel, "RadialMenu", "hovered")
            local names = { "Fire", "Ice", "Wind", "Earth", "Lightning", "Heal" }
            if i ~= nil and i >= 0 then cast(names[math.floor(i) + 1]) end
            cast_in = 1.1
        end
    elseif aiming then
        aiming = false
        Radial.set(wheel, "aim", nil)
    end
    if flash_t > 0 then
        flash_t = flash_t - dt
        local c = COLOURS[string.lower(cast_name)] or { 1, 1, 1 }
        Sprite.set_color(flash, c[1], c[2], c[3], math.max(0, flash_t / 0.35) * 0.9)
    end
    local h = Entity.get_custom(wheel, "RadialMenu", "hovered")
    local names = { "Fire", "Ice", "Wind", "Earth", "Lightning", "Heal" }
    local over = (h ~= nil and h >= 0) and names[math.floor(h) + 1] or "nothing"
    UI.set_text(status, "pointing at " .. over .. (cast_name ~= "" and ("   last cast: " .. cast_name) or "")
        .. (aiming and "   (the wheel turns itself)" or ""))
end
