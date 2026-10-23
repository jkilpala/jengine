-- Scene 83 - Animation frame events: a clip's frames drive the game
--
-- `AnimationFrame.triggers` has been authorable in the editor's animation
-- editor from the start, and every runtime computed what fired and then
-- threw it away (`let _sprite_events = ...`). So a sound placed on a frame
-- was silent, and `on_animation_event` was a handler name the metadata
-- accepted, the subscription index tracked, and nothing ever called.
--
-- Since Sep 2026 both halves work in every runtime: `PlaySound` plays, and
-- `EmitEvent` arrives here as
--
--   function on_animation_event(entity, ev)   -- ev.name, ev.data
--
-- This scene is the shape a fighting game is written in: the hitbox is live
-- on the frames the clip says it is, not on a timer the script keeps beside
-- the animation and hopes stays in step.

script = { tick_rate = 60, events = { "on_animation_event" } }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/83-frame-events.scene"

local fighter, hitbox, frame_label, log_label, count_label
local live = false
local hits, opens = 0, 0
local recent = {}
local looping = true
local speed = 1.0

function init(entity)
    fighter = entity
    hitbox = Entity.find_by_name("Hitbox")
    frame_label = Entity.find_by_name("Frame")
    log_label = Entity.find_by_name("Log")
    count_label = Entity.find_by_name("Count")
    log("[scene_83] ready - the clip's own frames open and close the hitbox")
end

local function note(line)
    table.insert(recent, 1, line)
    while #recent > 6 do
        table.remove(recent)
    end
    if log_label ~= nil then
        Text.set(log_label, table.concat(recent, "\n"))
    end
end

-- The whole point of the scene: the frames say when.
function on_animation_event(entity, ev)
    local name = ev and ev.name or "?"
    if name == "hitbox_on" then
        live = true
        opens = opens + 1
        local damage = (ev.data and ev.data.damage) or 0
        note(string.format("frame %d: hitbox_on (damage %d)",
            math.floor((ev.data and ev.data.frame) or 0), math.floor(damage)))
        -- A hit lands on the frame the clip opened on, not a frame later.
        hits = hits + 1
    elseif name == "hitbox_off" then
        live = false
        note(string.format("frame %d: hitbox_off", math.floor((ev.data and ev.data.frame) or 0)))
    else
        note("event: " .. tostring(name))
    end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)

    if Input.is_key_just_pressed("Space") then
        Animation.play(fighter, "punch")
    end
    if Input.is_key_just_pressed("L") then
        looping = not looping
        note(looping and "looping on" or "looping off (Space replays)")
        if not looping then
            Animation.stop(fighter)
        else
            Animation.play(fighter, "punch")
        end
    end
    if Input.is_key_just_pressed("1") then
        speed = math.max(0.25, speed - 0.25)
    end
    if Input.is_key_just_pressed("2") then
        speed = math.min(3.0, speed + 0.25)
    end

    -- The hitbox is drawn only while the frames say it is live. Nothing here
    -- counts frames or seconds: that is the claim.
    if hitbox ~= nil then
        local a = live and 0.55 or 0.0
        Sprite.set_color(hitbox, 1.0, 0.35, 0.3, a)
    end
    if frame_label ~= nil then
        Text.set(frame_label, live and "HITBOX LIVE" or "recovering")
    end
    if count_label ~= nil then
        Text.set(count_label, string.format("opened %d   hits %d   speed %.2fx   %s",
            opens, hits, speed, looping and "looping" or "one-shot"))
    end
end
