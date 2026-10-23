-- 131 Rack Focus: a forest drawn in layers - hills, pines, the deer, the
-- ferns by the lens - and `depth_of_field` blurring each by how far its z
-- is from `focus`. This script only moves the focus: it eases it from one
-- layer's z to the next, holding on each, the way a camera operator racks
-- focus between subjects. The ferns and mist move a little, so the soft
-- layers are seen to be soft and not just smudged.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/131-rack-focus.scene"
local LAYERS = { { 20, "the ferns by the lens" }, { 0, "the deer" }, { -20, "the pines" }, { -40, "the far hills" } }
local HOLD, PULL = 3.2, 1.4
local status, ferns_l, ferns_r, mist_far, mist_mid, deer
local captions, holding, dof_on = true, false, true
local t, phase = 0, 0
local at, from, to = 2, 0, 0 -- the layer focused on, and the pull in progress
local focus = 0

local function pull_to(i)
    from = focus
    at = i
    to = LAYERS[i][1]
    phase = 0
end

function init(entity)
    status = Entity.find_by_name("Status")
    ferns_l = Entity.find_by_name("FernsLeft")
    ferns_r = Entity.find_by_name("FernsRight")
    mist_far = Entity.find_by_name("MistFar")
    mist_mid = Entity.find_by_name("MistMid")
    deer = Entity.find_by_name("Deer")
    focus = LAYERS[at][1]
    from, to, phase = focus, focus, PULL + HOLD
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("Space") then holding = not holding end
    if Input.is_key_just_pressed("F") then
        dof_on = not dof_on
        PostProcess.set_enabled("depth_of_field", dof_on)
    end
    for i = 1, #LAYERS do
        if Input.is_key_just_pressed(tostring(i)) then pull_to(i) end
    end
    t = t + dt
    phase = phase + dt
    -- Hold on a layer, then pull to the next one - in, out, back.
    if not holding and phase > PULL + HOLD then
        pull_to(at % #LAYERS + 1)
    end
    local f = math.min(1, phase / PULL)
    f = f * f * (3 - 2 * f)
    focus = from + (to - from) * f
    PostProcess.set_float("depth_of_field", "focus", focus)
    -- The ferns stir, the mist drifts and the deer breathes.
    Transform.set_rotation(ferns_l, math.sin(t * 0.9) * 0.025)
    Transform.set_rotation(ferns_r, math.sin(t * 0.8 + 1.3) * 0.025)
    Transform.set_position(mist_far, math.sin(t * 0.05) * 120, -40)
    Transform.set_position(mist_mid, math.sin(t * 0.07 + 2) * 160, -110)
    Transform.set_position(deer, 80, -175 + math.sin(t * 0.6) * 2)
    local where = LAYERS[at][2]
    if f < 1 then
        UI.set_text(status, "pulling focus to " .. where)
    else
        UI.set_text(status, "focus on " .. where .. (holding and "  (held)" or ""))
    end
    if not dof_on then UI.set_text(status, "depth of field off - every layer sharp") end
end
