-- 156 Thunderhead: this storm's `lightning` component makes every bolt -
-- its branches grown from a seed, its flicker, the light it throws on the
-- sea and the tower. The script says where to strike (`Lightning.strike`)
-- and, told it struck (`on_strike`), plays the thunder late by how far off
-- it fell.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/156-thunderhead.scene"
local ROD_X, ROD_Y = 330.0, 286.0
local LAMP_X, LAMP_Y = 330.0, 201.0
local status
local captions = true
local storm, beam, beam_light
local t, next_in = 0, 1.0
local thunders = {}        -- { at, volume }
local strikes = 0
local aimed_x = 0

-- From somewhere in the cloud to a point on the sea, or now and then the rod.
local function strike_at(x, y)
    local from = x + (math.random() - 0.5) * 300
    Transform.set_position(storm, math.max(-600, math.min(600, from)), 250)
    Lightning.strike(storm, x, y)
    aimed_x = x
end

function init(entity)
    status = Entity.find_by_name("Status")
    storm = entity
    beam = Entity.find_by_name("Beam")
    beam_light = Entity.find_by_name("BeamLight")
    Audio.set_variation("sounds/th_thunder.wav", 3, 0.15)
    Audio.play_sound("sounds/th_rain.wav", 0.45, 1, 0, true)
    Audio.play_sound("sounds/cove_waves.wav", 0.3, 1, 0, true)
end

-- The bolt has struck: the thunder follows, later and quieter the further
-- from the middle it fell.
function on_strike(entity)
    strikes = strikes + 1
    local far = math.abs(aimed_x) / 1280
    table.insert(thunders, { at = t + 0.15 + far * 1.4, volume = 1.0 - far * 0.5 })
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    next_in = next_in - dt
    if next_in <= 0 then
        if math.random() < 0.25 then
            strike_at(ROD_X, ROD_Y)
        else
            strike_at((math.random() - 0.5) * 1150, -45 - math.random() * 60)
        end
        next_in = 1.2 + math.random() * 3.0
    end
    if Input.is_mouse_left_just_pressed() then
        local mx, my = Input.get_mouse_world_position()
        strike_at(mx, math.min(my, 150))
    end
    for i = #thunders, 1, -1 do
        if t >= thunders[i].at then
            Audio.play_sound("sounds/th_thunder.wav", thunders[i].volume)
            table.remove(thunders, i)
        end
    end
    -- The lamp turns: the beam swings out over the sea and round behind
    -- the tower, long when it points across the view, short when at us.
    local c = math.cos(t * 0.8)
    Transform.set_scale(beam, c, 1)
    Transform.set_rotation(beam_light, c < 0 and 0 or 3.14159)
    Lighting.set_intensity(beam_light, 2.5 * math.abs(c))
    UI.set_text(status, strikes .. " strikes")
end
