-- Scene 20 overlay — pushed on top of `scenes/20-pause-menu.scene` by P.
--
-- Three buttons:
--   Resume     -> Audio.resume_all(); Scene.pop()
--   Restart    -> Scene.load(main)   — hard reset, fade out first
--   Quit       -> Scene.load(hub)    — fade out first
--
-- P also acts as Resume (mirrors the typical pause-menu toggle UX).
-- We avoid ESC here because the editor uses it to stop play-mode.

script = { tick_rate = 60 }

local GUI_DOC  = "ui/scene_20_overlay.gui"
local MAIN = "scenes/20-pause-menu.scene"
local HUB  = "scenes/hub.scene"

-- The engine's one-frame edge. This overlay script is created fresh on
-- every push, so a local `prev_p = false` latch read the P that opened the
-- overlay - still held - as a new press and popped straight back: holding
-- P flickered pause on and off every frame. `is_key_just_pressed` is true
-- only on the frame the key goes down, and the overlay's first frame is
-- after that one.
local function edge_p()
    return Input.is_key_just_pressed("P")
end

-- Two-step fade-out: queue fade, then load the destination after
-- the duration has elapsed. Driven from update().
local pending_action = nil
local pending_t      = 0.0
local PENDING_DUR    = 0.3

local function queue_load(path)
    Scene.fade_out(PENDING_DUR)
    pending_action = path
    pending_t      = 0.0
end

function on_gui_click(self_entity, node_id)
    if node_id == "btn_resume" then
        log("[scene_20_overlay] resume")
        Audio.resume_all()
        Scene.pop()
    elseif node_id == "btn_restart" then
        log("[scene_20_overlay] restart")
        queue_load(MAIN)
    elseif node_id == "btn_quit" then
        log("[scene_20_overlay] quit -> hub")
        queue_load(HUB)
    end
end

function init(entity)
    GUI.on_click(entity, GUI_DOC, "btn_resume",  "on_gui_click")
    GUI.on_click(entity, GUI_DOC, "btn_restart", "on_gui_click")
    GUI.on_click(entity, GUI_DOC, "btn_quit",    "on_gui_click")
    -- Soft fade-in for the overlay itself (very short — just polish).
    Scene.fade_in(0.2)
    log("[scene_20_overlay] ready — Resume / Restart / Quit")
end

function update(entity, dt)
    if pending_action then
        pending_t = pending_t + dt
        if pending_t >= PENDING_DUR then
            -- Audio resumes regardless so the new scene starts clean.
            Audio.resume_all()
            local path = pending_action
            pending_action = nil
            Scene.load(path)
            return
        end
        return
    end

    -- P acts as Resume (toggle off).
    if edge_p() then
        log("[scene_20_overlay] P -> resume")
        Audio.resume_all()
        Scene.pop()
    end
end
