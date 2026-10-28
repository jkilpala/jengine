-- Scene 297: a normal-mapped mesh beside a normal-mapped sprite, under one
-- lamp circling over both - the mesh's bumps used to light flat - and the
-- lamp's pool squashed into an ellipse (`squash` on the light). Two lamps
-- stand in one place, one round and one squashed; S swaps which is lit.
-- N takes both normal maps away and back.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/297-mesh-normals.scene"
local line, mesh, sprite, round, squashed
local squash, normals, hold = false, true, false
local t = 0

function init(entity)
    line = Entity.find_by_name("Line0")
    mesh = Entity.find_by_name("MeshBumps")
    sprite = Entity.find_by_name("SpriteBumps")
    round = Entity.find_by_name("Lamp")
    squashed = Entity.find_by_name("LampSquashed")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("S") then
        squash = not squash
        Lighting.set_intensity(round, squash and 0 or 1.4)
        Lighting.set_intensity(squashed, squash and 1.4 or 0)
    end
    if Input.is_key_just_pressed("N") then
        normals = not normals
        local id = normals and "sprites/bumps128_n.png" or nil
        Material.set_normal_map(mesh, id)
        Material.set_normal_map(sprite, id)
    end
    if Input.is_key_just_pressed("Space") then hold = not hold end
    if not hold then t = t + dt end
    local x, y = math.cos(t * 0.6) * 420, -40 + math.sin(t * 1.2) * 120
    Transform.set_position(round, x, y)
    Transform.set_position(squashed, x, y)
    UI.set_text(line, "pool " .. (squash and "squashed (an ellipse)" or "round") .. "   normal maps "
        .. (normals and "on" or "off") .. (hold and "   (held)" or ""))
end
