-- 146 Curtain Call: four acts, each a scene of its own, each handing over
-- to the next with `Scene.transition(path, style, seconds, texture)` - the
-- engine keeps the act's last frame and wipes it away over the next scene,
-- in the style the act names. The Director's `Act` custom component says
-- which act this is.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/146-curtain-call.scene"
local ACTS = { { "scenes/curtain/act_2.scene", "Act I - the Enchanted Forest", "iris" }, { "scenes/curtain/act_3.scene", "Act II - a Night in the Desert", "diamond" }, { "scenes/curtain/act_4.scene", "Act III - Under the Sea", "slide" }, { "scenes/146-curtain-call.scene", "Act IV - the Winter Village", "dissolve" } }
local STYLES = { "fade", "iris", "diamond", "slide", "wipe", "dissolve" }
local MASK = "masks/curtain_dissolve.png"
local status, dancer, spot
local captions = true
local act = 1
local style
local t, leaving = 0, false

function init(entity)
    act = math.floor((Entity.get_custom(entity, "Act", "index") or 1) + 0.5)
    style = ACTS[act][3]
    status = Entity.find_by_name("Status")
    dancer = Entity.find_by_name("Dancer")
    spot = Entity.find_by_name("Spotlight")
end

local function next_act()
    if leaving then return end
    leaving = true
    local to = ACTS[act][1]
    local texture = ""
    if style == "dissolve" then texture = MASK end
    Scene.transition(to, style, 1.1, texture)
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    for k = 1, #STYLES do
        if Input.is_key_just_pressed(tostring(k)) then style = STYLES[k] end
    end
    if Input.is_key_just_pressed("Space") or t > 4.5 then next_act() end
    -- The dancer turns, steps and bows; the spotlight follows her.
    local x = math.sin(t * 1.1) * 220
    Transform.set_position(dancer, x, -200 + math.abs(math.sin(t * 3.0)) * 14)
    Transform.set_rotation(dancer, math.sin(t * 3.0) * 0.12)
    local face = 1
    if math.cos(t * 1.1) < 0 then face = -1 end
    Transform.set_scale(dancer, face, 1)
    Transform.set_position(spot, x, -150)
    UI.set_text(status, ACTS[act][2] .. "   - next, by " .. style)
end
