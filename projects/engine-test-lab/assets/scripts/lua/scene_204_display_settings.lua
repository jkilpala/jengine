-- Scene 204: display and cursor settings. Display.* and Cursor.* ask; the
-- runtime applies the change at the start of the next frame, and the
-- getters answer what the window really has.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/204-display-settings.scene"
local CAPS = { 0, 30, 60, 120 }
local CURSORS = { "default", "pointer", "crosshair", "image", "hidden" }
local lines, bars = {}, {}
local cap_i, cur_i, res_i = 1, 1, 0
local vsync, confined = true, false
local t, frames, fps_t, fps = 0, 0, 0, 0

local function yes(b) return b and "yes" or "no" end

local function show()
    local w, h = Display.get_size()
    local cap = Display.get_max_fps()
    UI.set_text(lines[1], string.format("%s   %d x %d   vsync %s   cap %s   measured %d fps   cursor %s%s",
        Display.get_mode(), math.floor(w + 0.5), math.floor(h + 0.5), Display.get_vsync() and "on" or "off",
        cap == 0 and "off" or tostring(math.floor(cap + 0.5)), fps, CURSORS[cur_i], confined and " (kept in)" or ""))
    local modes = Display.get_resolutions()
    local list = {}
    for i = 1, math.min(#modes, 5) do
        list[#list + 1] = string.format("%dx%d@%d", modes[i].w, modes[i].h, modes[i].hz)
    end
    UI.set_text(lines[2], "resolutions: " .. (#list > 0 and table.concat(list, "  ") or "(none)") .. (#modes > 5 and "  ..." or ""))
    UI.set_text(lines[3], "supports: fullscreen " .. yes(Display.supports("fullscreen")) .. "  exclusive " .. yes(Display.supports("exclusive"))
        .. "  window_size " .. yes(Display.supports("window_size")) .. "  vsync " .. yes(Display.supports("vsync"))
        .. "  max_fps " .. yes(Display.supports("max_fps")))
    UI.set_text(lines[4], "cursor_image " .. yes(Display.supports("cursor_image")) .. "  cursor_confine " .. yes(Display.supports("cursor_confine")))
end

local function set_cursor()
    local c = CURSORS[cur_i]
    if c == "hidden" then
        Cursor.set_visible(false)
        return
    end
    Cursor.set_visible(true)
    if c == "image" then Cursor.set_image("sprites/lab204_cursor.png", 1, 1) else Cursor.set_icon(c) end
end

function init(entity)
    for k = 0, 3 do lines[k + 1] = Entity.find_by_name("Line" .. k) end
    for i = 0, 5 do bars[i + 1] = Entity.find_by_name("Bar" .. i) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    frames = frames + 1
    fps_t = fps_t + dt
    if fps_t >= 0.5 then
        fps = math.floor(frames / fps_t + 0.5)
        frames, fps_t = 0, 0
    end
    if Input.is_key_just_pressed("1") then Display.set_mode("windowed") end
    if Input.is_key_just_pressed("2") then Display.set_mode("borderless") end
    if Input.is_key_just_pressed("3") then Display.set_mode("fullscreen") end
    if Input.is_key_just_pressed("R") then
        local modes = Display.get_resolutions()
        if #modes > 0 then
            res_i = res_i % #modes + 1
            Display.set_size(modes[res_i].w, modes[res_i].h)
        end
    end
    if Input.is_key_just_pressed("V") then
        vsync = not vsync
        Display.set_vsync(vsync)
    end
    if Input.is_key_just_pressed("F") then
        cap_i = cap_i % #CAPS + 1
        Display.set_max_fps(CAPS[cap_i])
    end
    if Input.is_key_just_pressed("C") then
        cur_i = cur_i % #CURSORS + 1
        set_cursor()
    end
    if Input.is_key_just_pressed("L") then
        confined = not confined
        Cursor.set_confined(confined)
    end
    -- Bars sweeping fast: tearing shows with vsync off, steps at a low cap.
    for i, b in ipairs(bars) do
        Transform.set_position(b, ((t * (900 + i * 120)) % 1300) - 650, 120 - (i - 1) * 36)
    end
    show()
end
