-- The showcase hub: a menu drawn from nav.SCENES into the scene's Row
-- slots, chosen with the keyboard, a pad or the mouse.
--
-- There are more scenes than slots, so the rows are a window onto the list
-- that scrolls to keep the chosen scene in view (with a couple of rows of
-- context past it), and the mouse wheel and PgUp / PgDn scroll it too.
-- `top` is the list index shown in Row1, less one.
--
-- The selection is a light: SelectLight and its glow ease to the chosen
-- row, so moving through the menu moves the light across the floor.

local nav = require("nav")

local ROW_Y0 = 160.0      -- tools/gen_scenes.py HUB_ROW_Y0
local ROW_STEP = 21.0     -- HUB_ROW_STEP
local ROW_X = -420.0      -- HUB_ROW_X
local SLOTS = 22          -- HUB_SLOTS
local ROW_HIT_W = 440.0   -- how far right of ROW_X a row answers the mouse

-- World text hangs down from its position, so a row's middle is this far
-- below its y. The light and glow aim at the middle.
local ROW_MID = 10.0

-- Rows kept visible past the chosen one when the window follows it.
local MARGIN = 2

-- Seconds after the hub opens before the mouse can choose a row.
local SETTLE = 0.4

local light_y = ROW_Y0 - ROW_MID
local IDLE = { 0.45, 0.46, 0.52 }
local CHOSEN = { 1.0, 0.86, 0.62 }

local rows = {}
local light, glow, blurb, track, thumb, count
local chosen = 1
local top = 0
local wheel_acc = 0.0

local t = 0.0

local function row_y(i)
    return ROW_Y0 - (i - 1) * ROW_STEP
end

local function max_top()
    return math.max(0, #nav.SCENES - SLOTS)
end

local function clamp_top(v)
    return math.max(0, math.min(max_top(), v))
end

-- Scroll just far enough that the chosen scene, and MARGIN rows past it,
-- are in view.
local function follow()
    local m = math.min(MARGIN, math.floor((SLOTS - 1) / 2))
    if chosen - 1 - m < top then top = chosen - 1 - m end
    if chosen + m > top + SLOTS then top = chosen + m - SLOTS end
    top = clamp_top(top)
end

local function paint_scrollbar()
    local n = #nav.SCENES
    local list_h = SLOTS * ROW_STEP
    local show = n > SLOTS
    if track then Sprite.set_color(track, 0.30, 0.31, 0.36, show and 0.6 or 0.0) end
    if thumb then
        if show then
            local h = math.max(12.0, list_h * SLOTS / n)
            local y = ROW_Y0 - (list_h - h) * top / max_top()
            Transform.set_position(thumb, ROW_X - 22, y)
            Transform.set_scale(thumb, 1.0, h)
            Sprite.set_color(thumb, 1.0, 0.80, 0.52, 0.9)
        else
            Sprite.set_color(thumb, 1.0, 0.80, 0.52, 0.0)
        end
    end
    if count then
        Text.set(count, n > 0 and string.format("%d / %d", chosen, n) or "")
    end
end

local function paint()
    for slot, e in ipairs(rows) do
        local i = top + slot
        local s = nav.SCENES[i]
        if s then
            local c = (i == chosen) and CHOSEN or IDLE
            Text.set(e, s.title)
            Text.set_color(e, c[1], c[2], c[3], 1.0)
        else
            Text.set(e, "")
        end
    end
    local s = nav.SCENES[chosen]
    if blurb then Text.set(blurb, s and s.blurb or "") end
    paint_scrollbar()
    nav.remember_hub(top)
end

local function choose(i)
    local n = #nav.SCENES
    if n == 0 then return end
    i = (i - 1) % n + 1
    if i ~= chosen then
        chosen = i
        follow()
        paint()
    end
end

-- Move the window without following: the chosen scene is pulled along only
-- if it would leave the view. Returns nothing; repaints.
local function scroll(by)
    local was = top
    top = clamp_top(top + by)
    if top == was then return end
    if chosen <= top then chosen = top + 1 end
    if chosen > top + SLOTS then chosen = top + SLOTS end
    paint()
end

local function open()
    local s = nav.SCENES[chosen]
    if s then Scene.load(s.path) end
end

-- The row under the mouse, or nil.
local function row_at_mouse()
    local wx, wy = Input.get_mouse_world_position()
    if not wx then return nil end
    if wx < ROW_X - 20 or wx > ROW_X + ROW_HIT_W then return nil end
    for slot = 1, math.min(#nav.SCENES - top, SLOTS) do
        if math.abs(wy - (row_y(slot) - ROW_MID)) < ROW_STEP * 0.5 then return top + slot end
    end
    return nil
end

local last_mx, last_my = nil, nil

function init(entity)
    for i = 1, SLOTS do
        rows[i] = Entity.find_by_name("Row" .. i)
    end
    light = Entity.find_by_name("SelectLight")
    glow = Entity.find_by_name("SelectGlow")
    blurb = Entity.find_by_name("Blurb")
    track = Entity.find_by_name("ScrollTrack")
    thumb = Entity.find_by_name("ScrollThumb")
    count = Entity.find_by_name("ScrollCount")
    -- Back from a scene: the list as it was, on the scene just left (PgUp /
    -- PgDn inside the scenes may have walked away from the one opened, so
    -- the window then follows it).
    local was_top, left = nav.hub_memory()
    if was_top then top = clamp_top(was_top) end
    if left then chosen = left end
    follow()
    light_y = row_y(chosen - top) - ROW_MID
    paint()
end

function update(entity, dt)
    t = t + dt

    if Input.is_action_just_pressed("move_down") then choose(chosen + 1) end
    if Input.is_action_just_pressed("move_up") then choose(chosen - 1) end
    -- PgUp / PgDn (the bumpers on a pad) page; Home / End go to either end.
    local page = SLOTS - 1
    if Input.is_action_just_pressed("next_scene") then
        choose(math.min(#nav.SCENES, chosen + page))
    end
    if Input.is_action_just_pressed("prev_scene") then
        choose(math.max(1, chosen - page))
    end
    if Input.is_key_just_pressed("Home") then choose(1) end
    if Input.is_key_just_pressed("End") then choose(#nav.SCENES) end

    -- The wheel scrolls three rows a notch; positive is away from you, up
    -- the list. A trackpad sends fractions, so they add up first.
    wheel_acc = wheel_acc + (tonumber(Input.get_mouse_wheel()) or 0) * 3.0
    local steps = (wheel_acc >= 0) and math.floor(wheel_acc) or -math.floor(-wheel_acc)
    if steps ~= 0 then
        wheel_acc = wheel_acc - steps
        scroll(-steps)
    end

    -- The mouse picks a row only when it moves, so it does not fight the
    -- keyboard by resting over another row. A freshly loaded scene does not
    -- know where the pointer is for its first frames - it reads (0, 0) and
    -- then jumps to the real place, which looked like a move and selected
    -- whatever row the pointer was resting on, instead of the scene just
    -- left. So the first moments only note where it is, and after that it
    -- has to move a few pixels.
    local mx, my = Input.get_mouse_position()
    mx, my = tonumber(mx) or 0, tonumber(my) or 0
    if last_mx == nil or t < SETTLE then last_mx, last_my = mx, my end
    if math.abs(mx - last_mx) + math.abs(my - last_my) > 3 then
        last_mx, last_my = mx, my
        local hit = row_at_mouse()
        if hit then choose(hit) end
    end
    if Input.is_mouse_left_just_pressed() then
        local hit = row_at_mouse()
        if hit then
            chosen = hit
            open()
            return
        end
    end
    if Input.is_action_just_pressed("confirm") then
        open()
        return
    end

    -- Ease the light to the chosen row, and let it breathe a little.
    local target = row_y(chosen - top) - ROW_MID
    light_y = light_y + (target - light_y) * math.min(1.0, dt * 10.0)
    local sway = math.sin(t * 1.3) * 12.0
    if light then
        Transform.set_position(light, ROW_X + 170 + sway, light_y)
        Lighting.set_intensity(light, 2.1 + 0.15 * math.sin(t * 2.7))
    end
    if glow then
        Transform.set_position(glow, ROW_X + 180 + sway * 0.5, light_y)
    end
end
