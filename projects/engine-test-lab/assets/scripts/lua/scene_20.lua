-- Scene 20 - Pause Menu (fade + push_scene + audio pause)
--
-- Demonstrates the Slice 12.A `fade_in` / `fade_out` script API, the
-- `pause_all_sounds` / `resume_all_sounds` helpers, and `push_scene`
-- as a non-destructive pause overlay.
--
-- Controls:
--   P         pause (mute audio, push the overlay scene). Not Esc: nav.lua
--             owns Esc for the hub, and the hint used to say ESC anyway.
--   M         toggle background music (hum loop)
--   H / Esc   hub (nav.lua)
--   PgUp/PgDn prev / next scene (nav.lua)
--
-- On `init` we kick a 0.4s `fade_in` so the scene reveals from black.
-- When the overlay pops itself, this scene resumes ticking and the
-- audio is un-muted by the overlay script.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH    = "scenes/20-pause-menu.scene"
local OVERLAY_PATH  = "scenes/20-pause-overlay.scene"

local HUM = "sounds/hum.wav"

local status_id
local music_state = "stopped"
local t = 0.0

-- Bouncer state — kept in-script so push_scene snapshot/restore
-- preserves the live position via component state.
local bouncer_id
local vel_x, vel_y = 220.0, 160.0
local BOUND_X = 540.0
local BOUND_Y = 280.0

-- The engine's own one-frame edge, not a local `prev` table. A local
-- latch is reset whenever this script is re-created, and a key still held
-- then reads as a fresh press: P paused, the overlay popped on the same
-- press, and holding P toggled pause every frame.
local function edge(key)
    return Input.is_key_just_pressed(key)
end

local function refresh_status()
    if status_id and status_id ~= 0 then
        Text.set(status_id, "music: " .. music_state)
    end
end

function init(entity)
    Scene.fade_in(0.4)
    log("[scene_20] ready — P pauses (push overlay), M toggles music")
end

function update(entity, dt)
    if status_id == nil then
        status_id  = Entity.find_by_name("Status")
        bouncer_id = Entity.find_by_name("Bouncer")
        refresh_status()
    end

    -- Bounce the sprite inside the screen rect.
    if bouncer_id and bouncer_id ~= 0 then
        local x, y = Transform.get_position(bouncer_id)
        x = x + vel_x * dt
        y = y + vel_y * dt
        if x >  BOUND_X then x =  BOUND_X; vel_x = -math.abs(vel_x) end
        if x < -BOUND_X then x = -BOUND_X; vel_x =  math.abs(vel_x) end
        if y >  BOUND_Y then y =  BOUND_Y; vel_y = -math.abs(vel_y) end
        if y < -BOUND_Y then y = -BOUND_Y; vel_y =  math.abs(vel_y) end
        Transform.set_position(bouncer_id, x, y)
    end

    if edge("P") then
        log("[scene_20] pause -> push overlay, pause_all_sounds()")
        Audio.pause_all()
        Scene.push(OVERLAY_PATH)
        return
    end

    if edge("M") then
        if music_state == "playing" then
            Audio.stop_music()
            music_state = "stopped"
        else
            Audio.play_music(HUM, 0.4, true)
            music_state = "playing"
        end
        refresh_status()
    end

    nav.tick(SCENE_PATH)
end
