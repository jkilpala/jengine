-- Scene 226 - live ops
--
-- Every value here comes from LiveOps.get: the lab's live ops service's,
-- worked out for this install, or - before it answers, offline, or with no
-- service running - the defaults in settings.liveops.defaults. The shape
-- spins at lab226_speed (faster while the lab226_weekend event is on), is
-- tinted lab226_tint, and is a square or a diamond by the install's
-- lab226_shape experiment variant. F asks the service again now.
--
-- It also says something of itself: the attribute lab=226 (a rule on
-- lab226_said answers it), and the player's language (lab226_greeting has a
-- Finnish rule). Once the variant is known it reports the exposure, and E
-- counts a "tap" in the experiment - both read at the service's
-- /admin/api/liveops/channel/<name>/results.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/226-live-ops.scene"

local status_ui, detail_ui, greeting_ui, values_ui, events_ui, shape
local angle = 0
local exposed = false
local taps = 0

function init(entity)
    status_ui = Entity.find_by_name("Status")
    detail_ui = Entity.find_by_name("Detail")
    greeting_ui = Entity.find_by_name("Greeting")
    values_ui = Entity.find_by_name("Values")
    events_ui = Entity.find_by_name("Events")
    shape = Entity.find_by_name("Shape")
    angle = 0
    exposed = false
    taps = 0
    LiveOps.set_attribute("lab", 226)
end

-- Whole seconds as "1h 05m" / "4m 10s": numbers from the service are
-- floats in Fengari, so nothing here formats one with %d.
local function span(secs)
    local s = math.floor(secs + 0.5)
    if s >= 3600 then
        return math.floor(s / 3600) .. "h " .. string.format("%02.0f", math.floor((s % 3600) / 60)) .. "m"
    end
    return math.floor(s / 60) .. "m " .. string.format("%02.0f", s % 60) .. "s"
end

function update(entity, dt)
    if Input.is_key_just_pressed("F") then
        LiveOps.fetch()
    end
    if Input.is_key_just_pressed("E") and LiveOps.expose("lab226_shape", "tap", 1) then
        taps = taps + 1
    end

    local speed = LiveOps.get("lab226_speed", 1)
    local diamond = LiveOps.variant("lab226_shape") == "diamond"
    angle = angle + dt * speed
    Transform.set_rotation(shape, angle + (diamond and 0.785398 or 0))
    local tint = LiveOps.get("lab226_tint", { 1, 1, 1 })
    Sprite.set_color(shape, tint[1] or 1, tint[2] or 1, tint[3] or 1, 1)

    local status = "live ops: " .. LiveOps.status()
    if LiveOps.fetching() then
        status = status .. " (asking...)"
    end
    Text.set(status_ui, status .. "   revision " .. string.format("%.0f", LiveOps.revision()) .. "   release " .. string.format("%.0f", LiveOps.release()))
    local err = LiveOps.error()
    if #err > 110 then
        err = string.sub(err, 1, 107) .. "..."
    end
    Text.set(detail_ui, err ~= "" and ("last problem: " .. err) or "")
    Text.set(greeting_ui, tostring(LiveOps.get("lab226_greeting", "(no greeting)"))
        .. "   (" .. tostring(LiveOps.get("lab226_said", "no lab226_said")) .. ")")

    local variant = LiveOps.variant("lab226_shape")
    if not exposed and variant ~= "" then
        exposed = LiveOps.expose("lab226_shape")
    end
    Text.set(values_ui, "lab226_speed " .. string.format("%.2f", speed)
        .. "   lab226_tint " .. string.format("%.2f %.2f %.2f", tint[1] or 1, tint[2] or 1, tint[3] or 1)
        .. "   experiment lab226_shape: " .. (variant ~= "" and variant or "(not in it)")
        .. "   keys: " .. #LiveOps.keys()
        .. "   exposed: " .. (exposed and "yes" or "no") .. "   taps: " .. taps)

    local lines = {}
    for _, id in ipairs(LiveOps.events()) do
        lines[#lines + 1] = LiveOps.event_name(id) .. " is on - ends in " .. span(LiveOps.event_ends(id))
    end
    local soon = LiveOps.event_starts("lab226_soon")
    if soon > 0 then
        lines[#lines + 1] = LiveOps.event_name("lab226_soon") .. " starts in " .. span(soon)
    end
    Text.set(events_ui, #lines > 0 and table.concat(lines, "   ") or "no live events")
    nav.tick(SCENE_PATH)
end
