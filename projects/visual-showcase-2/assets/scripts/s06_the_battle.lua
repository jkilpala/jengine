-- 06 The Battle: a side-on battle view on a 3D stage - the party on the
-- right, three monsters on the left, a ruin behind. Space casts the next
-- spell in turn (fire, frost, light) at the next monster: a burst of
-- sparks in the spell's colour, a flash of light over the stage, the
-- monster's flinch and a shake. The camera drifts slowly, as the
-- genre's does between turns, and depth of field holds the line of battle sharp.

local nav = require("nav")
local explore = require("explore")

local SCENE = "scenes/06-the-battle.scene"
local SPELLS = {
    { name = "fire", light = { 1.0, 0.55, 0.2 }, start = { 6.0, 3.0, 1.0, 1.0 }, stop = { 2.0, 0.4, 0.1, 0.0 } },
    { name = "frost", light = { 0.5, 0.75, 1.0 }, start = { 1.5, 3.0, 6.0, 1.0 }, stop = { 0.2, 0.6, 2.0, 0.0 } },
    { name = "light", light = { 1.0, 0.95, 0.7 }, start = { 6.0, 6.0, 4.0, 1.0 }, stop = { 2.0, 2.0, 1.0, 0.0 } },
}
local FOES = { "toad", "golem", "wraith" }
local status, stage
local post = explore.post_all_on()
local captions = true
local foes, bursts, flashes = {}, {}, {}
local flash_t = { 0, 0, 0 }
local spell, target = 0, 0
local t = 0
local braziers = {}
local last = "Space casts"
-- The camera's rest, drifted about; a cast pushes in and eases back.
local YAW, PITCH, ZOOM = -0.22, 0.4, 48.0
local punch = 0

function init(entity)
    status = Entity.find_by_name("Status")
    stage = Entity.find_by_name("Stage")
    for i = 0, 2 do
        foes[i + 1] = Entity.find_by_name("Foe" .. i)
        bursts[i + 1] = Entity.find_by_name("Foe" .. i .. "Burst")
        flashes[i + 1] = Entity.find_by_name("Foe" .. i .. "Flash")
    end
    for i = 0, 1 do braziers[#braziers + 1] = Entity.find_by_name("Brazier" .. i .. "Light") end
end

local function cast()
    spell = spell % #SPELLS + 1
    target = target % #foes + 1
    local s = SPELLS[spell]
    local b = bursts[target]
    Particles.set(b, "color_start", s.start)
    Particles.set(b, "color_end", s.stop)
    Particles.emit_burst(b, 90)
    Lighting.set_color(flashes[target], s.light[1], s.light[2], s.light[3])
    flash_t[target] = 1.0
    Animation.play(foes[target], "hurt")
    Camera.shake(5, 0.25, 22)
    punch = 1.0
    return s.name .. " on the " .. FOES[target]
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions) end
    explore.post_keys(post)
    if Input.is_key_just_pressed("Space") or Input.is_action_just_pressed("confirm") then
        last = cast()
    end
    for i = 1, #foes do
        if flash_t[i] > 0 then
            flash_t[i] = math.max(0, flash_t[i] - dt * 1.6)
            Lighting.set_intensity(flashes[i], 6.0 * flash_t[i] * flash_t[i])
            if flash_t[i] == 0 then Animation.play(foes[i], "idle") end
        end
    end
    for i, l in ipairs(braziers) do
        Lighting.set_intensity(l, 2.2 + 0.3 * math.sin(t * 9.0 + i * 2.0))
    end
    -- The camera drifts, and leans in on a spell.
    punch = math.max(0, punch - dt * 1.4)
    if stage then
        Stage.set(stage, "yaw", YAW + math.sin(t * 0.15) * 0.12)
        Stage.set(stage, "pitch", PITCH + math.sin(t * 0.11) * 0.035)
        Stage.set(stage, "zoom", ZOOM * (1 + 0.06 * punch * punch))
    end
    if status then UI.set_text(status, last .. "      " .. explore.post_status(post)) end
end
