-- 155 Unit Select: every soldier has a `sprite_outline`, off until it
-- matters. The script decides who is selected and who is under the mouse,
-- and turns the outline on in a colour - `Outline.set(e, field, value)` -
-- the engine rings each soldier in his own shape: shield, spear and bow.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/155-unit-select.scene"
local BLUE = { { -420, 150 }, { -368, 150 }, { -316, 150 }, { -264, 150 }, { -420, 98 }, { -368, 98 }, { -316, 98 }, { -264, 98 }, { -420, 46 }, { -368, 46 }, { -316, 46 }, { -264, 46 }, { -380, -150 }, { -328, -150 }, { -276, -150 }, { -380, -202 }, { -328, -202 }, { -276, -202 } }
local RED = { { 470, 240 }, { 470, 180 }, { 470, 120 }, { 470, 60 }, { 470, 0 }, { 470, -60 }, { 470, -120 }, { 470, -180 } }
local SPEED = 120
-- Luau has atan2; Lua 5.3 (the editor, the web) has a two-argument atan.
local atan2 = math.atan2 or math.atan
local status
local captions = true
local units = {}           -- every soldier: e, shadow, x, y, tx, ty, blue
local boxfill, edges = nil, {}
local flag, flag_t = nil, 0
local dragging, sx, sy = false, 0, 0
local hovered

-- What each soldier's ring shows now, so it is set only when it changes.
local function ring(u, look)
    if u.look == look then return end
    u.look = look
    if look == "none" then
        Outline.set(u.e, "enabled", false)
        return
    end
    Outline.set(u.e, "enabled", true)
    if look == "selected" then
        Outline.set(u.e, "color", { 1.0, 0.78, 0.2, 1.0 })
        Outline.set(u.e, "width", 2.5)
        Outline.set(u.e, "glow", 8)
        Outline.set(u.e, "pulse", 1.4)
    elseif look == "hover" then
        Outline.set(u.e, "color", { 0.9, 0.95, 1.0, 0.85 })
        Outline.set(u.e, "width", 1.5)
        Outline.set(u.e, "glow", 3)
        Outline.set(u.e, "pulse", 0)
    elseif look == "target" then
        Outline.set(u.e, "color", { 1.0, 0.25, 0.2, 1.0 })
        Outline.set(u.e, "width", 2)
        Outline.set(u.e, "glow", 9)
        Outline.set(u.e, "pulse", 2.5)
    end
end

local function box(x0, y0, x1, y1)
    local cx, cy, w, h = (x0 + x1) / 2, (y0 + y1) / 2, math.abs(x1 - x0), math.abs(y1 - y0)
    Transform.set_position(boxfill, cx, cy)
    Transform.set_scale(boxfill, w, h)
    local t = 2
    local spots = { { cx, cy + h / 2, w, t }, { cx, cy - h / 2, w, t }, { cx - w / 2, cy, t, h }, { cx + w / 2, cy, t, h } }
    for k, sp in ipairs(spots) do
        Transform.set_position(edges[k], sp[1], sp[2])
        Transform.set_scale(edges[k], sp[3], sp[4])
    end
end

local function hide_box()
    Transform.set_position(boxfill, 0, -2000)
    for _, e in ipairs(edges) do Transform.set_position(e, 0, -2000) end
end

local function under(mx, my)
    for _, u in ipairs(units) do
        if (mx - u.x) ^ 2 + (my - u.y) ^ 2 < 22 * 22 then return u end
    end
end

local function count_selected()
    local n = 0
    for _, u in ipairs(units) do if u.selected then n = n + 1 end end
    return n
end

function init(entity)
    status = Entity.find_by_name("Status")
    boxfill = Entity.find_by_name("BoxFill")
    for k = 0, 3 do table.insert(edges, Entity.find_by_name("BoxEdge" .. k)) end
    flag = Entity.find_by_name("Flag")
    for i, p in ipairs(BLUE) do
        table.insert(units, { e = Entity.find_by_name("Blue" .. (i - 1)), shadow = Entity.find_by_name("Shadow" .. (i - 1)),
            x = p[1], y = p[2], tx = p[1], ty = p[2], blue = true, look = "none" })
    end
    for i, p in ipairs(RED) do
        table.insert(units, { e = Entity.find_by_name("Red" .. (i - 1)), shadow = Entity.find_by_name("RedShadow" .. (i - 1)),
            x = p[1], y = p[2], tx = p[1], ty = p[2], blue = false, look = "none" })
    end
    -- One squad starts selected, so the rings are seen at once.
    for i = 1, 4 do units[i].selected = true end
end

local function march(mx, my)
    local chosen = {}
    for _, u in ipairs(units) do if u.selected then table.insert(chosen, u) end end
    local n = #chosen
    if n == 0 then return end
    local cols = math.ceil(math.sqrt(n))
    for i, u in ipairs(chosen) do
        local c, r = (i - 1) % cols, math.floor((i - 1) / cols)
        u.tx = mx + (c - (cols - 1) / 2) * 50
        u.ty = my - (r - (math.ceil(n / cols) - 1) / 2) * 50
    end
    Transform.set_position(flag, mx, my + 10)
    flag_t = 1.5
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    local mx, my = Input.get_mouse_world_position()
    if Input.is_mouse_left_just_pressed() then dragging, sx, sy = true, mx, my end
    if dragging and Input.is_mouse_left_pressed() then
        if math.abs(mx - sx) > 6 or math.abs(my - sy) > 6 then box(sx, sy, mx, my) end
    end
    if dragging and Input.is_mouse_left_just_released() then
        dragging = false
        hide_box()
        local dragged = math.abs(mx - sx) > 6 or math.abs(my - sy) > 6
        local picked = under(mx, my)
        for _, u in ipairs(units) do
            if u.blue then
                if dragged then
                    u.selected = u.x > math.min(sx, mx) and u.x < math.max(sx, mx) and u.y > math.min(sy, my) and u.y < math.max(sy, my)
                else
                    u.selected = (u == picked)
                end
            end
        end
    end
    if Input.is_mouse_right_just_pressed() then march(mx, my) end
    hovered = under(mx, my)
    for _, u in ipairs(units) do
        local dx, dy = u.tx - u.x, u.ty - u.y
        local d = math.sqrt(dx * dx + dy * dy)
        if d > 1 then
            local step = math.min(d, SPEED * dt)
            u.x, u.y = u.x + dx / d * step, u.y + dy / d * step
            Transform.set_rotation(u.e, atan2(dy, dx))
        end
        Transform.set_position(u.e, u.x, u.y)
        Transform.set_position(u.shadow, u.x + 5, u.y - 6)
        local look = "none"
        if u.blue and u.selected then look = "selected"
        elseif u == hovered then look = u.blue and "hover" or (count_selected() > 0 and "target" or "hover") end
        ring(u, look)
    end
    if flag_t > 0 then
        flag_t = flag_t - dt
        Sprite.set_color(flag, 1, 1, 1, math.max(0, math.min(1, flag_t)))
    end
    UI.set_text(status, count_selected() .. " selected")
end
