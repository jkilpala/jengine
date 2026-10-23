-- Scene 295: the web portal (Platform.*, docs/web-platforms.md). Natively, in
-- the editor and in a web build for no portal the calls do nothing and an ad
-- answers at once, not shown; a build made for a portal (`jengine web-build
-- --platform poki`, and so on) talks to that portal's SDK, which holds the
-- game while an ad plays. The readout lists what came back.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/295-web-platform.scene"
local readout, events
local lines = {}

local function say(text)
    table.insert(lines, 1, text)
    if #lines > 8 then table.remove(lines) end
    Text.set(events, table.concat(lines, "\n"))
end

function init(entity)
    readout = Entity.find_by_name("Readout")
    events = Entity.find_by_name("Events")
    Text.set(readout, "Platform.name() = " .. tostring(Platform.name()))
    Event.on("platform_ad_finished", entity)
    Event.on("platform_pause", entity)
    Event.on("platform_resume", entity)
end

function on_platform_ad_finished(entity, data)
    say("ad finished: " .. tostring(data.kind) .. ", shown " .. tostring(data.shown)
        .. ", rewarded " .. tostring(data.rewarded))
end

function on_platform_pause(entity, data)
    say("paused by the portal")
end

function on_platform_resume(entity, data)
    say("resumed by the portal")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("1") then Platform.gameplay_start(); say("gameplay_start") end
    if Input.is_key_just_pressed("2") then Platform.gameplay_stop(); say("gameplay_stop") end
    if Input.is_key_just_pressed("3") then say("asked for a midgame ad"); Platform.show_ad("midgame") end
    if Input.is_key_just_pressed("4") then say("asked for a rewarded ad"); Platform.show_ad("rewarded", "test-lab-reward") end
    if Input.is_key_just_pressed("5") then Platform.happy_time(); say("happy_time") end
    if Input.is_key_just_pressed("6") then Platform.submit_score(100, "high-scores"); say("submit_score 100") end
    if Input.is_key_just_pressed("7") then Platform.game_ready(); say("game_ready") end
end
