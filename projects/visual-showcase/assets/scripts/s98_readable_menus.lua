-- 98 Readable Menus: a title screen a screen reader reads and works. The
-- menu is a .gui document; nothing here talks to the screen reader except
-- `Accessibility.announce` when something is pressed - the engine gathers
-- the menu into the platform's accessibility tree by itself. The pill at
-- the foot prints `Accessibility.speech()`, what the reader is saying, so
-- a screenshot shows it. Wait a few seconds and the demo walks the menu.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/98-readable-menus.scene"
local MENU, SPEECH = "ui/rm_menu.gui", "ui/rm_speech.gui"
local DEMO = { "btn_new", "btn_continue", "btn_settings", "music", "voices", "subtitles", "large_text",
    "reduce_motion", "btn_credits", "btn_quit" }
local SAY = {
    btn_new = "A new journey begins - in a real game, anyway",
    btn_continue = "There is no journey to continue yet",
    btn_settings = "Settings are on the right",
    btn_credits = "Made with jEngine",
    btn_quit = "Quit does nothing in a showcase",
}
local TOGGLES = { subtitles = "Subtitles", large_text = "Large text", reduce_motion = "Reduce motion" }

local captions = true
local quiet, demo_t, demo_i = 99.0, 0.0, 0
local announced, announced_t = "", 0.0
local backdrop
local t = 0.0
local on = { subtitles = true, large_text = false, reduce_motion = false }
local scale, scale_to = 1.0, 1.0

local function say(text)
    Accessibility.announce(text)
    announced, announced_t = text, 2.4
end

function init(entity)
    backdrop = Entity.find_by_name("Backdrop")
    for id, _ in pairs(SAY) do GUI.on_click(entity, MENU, id, "on_menu") end
    for id, _ in pairs(TOGGLES) do GUI.on_click(entity, MENU, id, "on_menu") end
    GUI.focus(MENU, "btn_new")
end

function on_menu(entity, id)
    quiet = 0
    if TOGGLES[id] then
        on[id] = not on[id]
        GUI.set(MENU, id, "checked", on[id])
        if id == "large_text" then scale_to = on[id] and 1.08 or 1.0 end
        say(TOGGLES[id] .. (on[id] and " on" or " off"))
    elseif SAY[id] then
        say(SAY[id])
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    local pressed = Input.get_just_pressed()
    -- (Not the first moments: the key that opened the scene is still down.)
    if pressed and #pressed > 0 and t > 0.5 then quiet = 0 end
    quiet = quiet + dt
    if Input.is_key_just_pressed("C") then captions = nav.toggle_ui_captions(captions) end

    -- The demo: after a few quiet seconds, focus walks the menu.
    if quiet > 6 then
        demo_t = demo_t + dt
        if demo_t > 1.8 then
            demo_t = 0
            demo_i = demo_i % #DEMO + 1
            GUI.focus(MENU, DEMO[demo_i])
        end
    end

    -- Large text: both cards grow (scale is inherited by all on them).
    scale = scale + (scale_to - scale) * math.min(1, dt * 8)
    GUI.set(MENU, "main", "scale", scale)
    GUI.set(MENU, "settings", "scale", scale)

    -- Reduce motion: the backdrop stops drifting.
    if not on.reduce_motion then
        Transform.set_position(backdrop, math.sin(t * 0.07) * 50, math.sin(t * 0.05) * 18)
    end

    -- The pill: an announcement for a moment, else what focus reads as.
    announced_t = announced_t - dt
    local text = Accessibility.speech()
    if announced_t > 0 then text = announced end
    if text == "" then text = "(nothing focused - press an arrow or Tab)" end
    GUI.set(SPEECH, "speech", "text", text)
end
