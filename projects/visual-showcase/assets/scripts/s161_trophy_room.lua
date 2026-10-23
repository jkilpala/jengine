-- 161 Trophy Room: two `toast_stack`s. Clicking a trophy unlocks its
-- achievement - `Toast.show(stack, title, text, icon)` - and everyday
-- notices come along on their own; the engine slides each in from its
-- corner, stacks them, waits and slides them away. This script is on the
-- achievements stack, so `on_toast` reaches it as each one appears.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/161-trophy-room.scene"
local ICONS = "sprites/trophyroom/icon_"
local TROPHIES = {
    { "Cup1", "Champion", "Won the summer tournament without dropping a round.", "cup", -420, 190 },
    { "Star1", "Shining Star", "Earned a perfect score.", "star", -140, 190 },
    { "Shield1", "Guardian", "Held the gate for a whole night.", "cup", 140, 190 },
    { "Medal1", "Medalist", "Finished in the top three, ten times.", "star", 420, 190 },
    { "Medal2", "Runner-Up", "Second, but never far behind.", "star", -280, 60 },
    { "Cup2", "Plucky", "Took part in fifty matches.", "cup", 0, 60 },
    { "Star2", "Collector", "Found every one of the seven hidden shells.", "gem", 280, 60 },
    { "Shield2", "Steadfast", "Played every day for a month.", "cup", -140, -70 },
    { "Cup3", "Grand Master", "Unlocked every other achievement in the room.", "star", 140, -70 },
}
local NOTES = {
    { "Ada is online", "She is in the lobby.", "friend" },
    { "Item found", "A rusty key, in the old well.", "gem" },
    { "New message", "Your guild has a quest for you.", "scroll" },
    { "Daily reward", "Fifty gold coins.", "bell" },
    { "Bo is online", "Playing The Long Corridor.", "friend" },
}
local status
local captions = true
local achievements, notices
local unlocked, count = {}, 0
local next_note, note_i = 1.5, 1
local next_unlock = 1.0
local shown = 0

local function unlock(i)
    local t = TROPHIES[i]
    if unlocked[i] then return end
    unlocked[i] = true
    count = count + 1
    local e = Entity.find_by_name(t[1])
    Outline.set(e, "enabled", true)
    Toast.show(achievements, "Unlocked: " .. t[2], t[3], ICONS .. t[4] .. ".png")
end

function init(entity)
    status = Entity.find_by_name("Status")
    achievements = entity
    notices = Entity.find_by_name("Notices")
end

function on_toast(entity)
    shown = shown + 1
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_mouse_left_just_pressed() then
        local mx, my = Input.get_mouse_world_position()
        for i, t in ipairs(TROPHIES) do
            if math.abs(mx - t[5]) < 44 and my > t[6] and my < t[6] + 102 then
                unlock(i)
                next_unlock = 3.0
            end
        end
    end
    next_note = next_note - dt
    if next_note <= 0 then
        local n = NOTES[note_i]
        Toast.show(notices, n[1], n[2], ICONS .. n[3] .. ".png")
        note_i = note_i % #NOTES + 1
        next_note = 2.6 + math.random() * 1.6
    end
    -- Without a click for a while, the next trophy unlocks by itself.
    next_unlock = next_unlock - dt
    if next_unlock <= 0 and count < #TROPHIES then
        for i = 1, #TROPHIES do
            if not unlocked[i] then unlock(i); break end
        end
        next_unlock = 3.0
    end
    UI.set_text(status, count .. " of " .. #TROPHIES .. " unlocked")
end
