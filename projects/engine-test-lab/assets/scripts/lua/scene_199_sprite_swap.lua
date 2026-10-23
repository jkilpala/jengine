-- Scene 199: sprite swapping. The totem's root carries a sprite_library
-- (top / middle / base, looks a / b / c); each part a sprite_resolver.
-- Sprite.set_label on the root changes the whole look; on a part, one part.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/199-sprite-swap.scene"
local LOOKS = { "a", "b", "c" }
local totem, status
local parts = {}
local worn = { top = 1, middle = 1, base = 1 }

local function show()
    if status then
        UI.set_text(status, "top " .. LOOKS[worn.top] .. "   middle " .. LOOKS[worn.middle] .. "   base " .. LOOKS[worn.base])
    end
end

function init(entity)
    totem = Entity.find_by_name("Totem")
    status = Entity.find_by_name("Status")
    parts.top = Entity.find_by_name("PartTop")
    parts.middle = Entity.find_by_name("PartMiddle")
    parts.base = Entity.find_by_name("PartBase")
    show()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    for i, key in ipairs({ "A", "B", "C" }) do
        if Input.is_key_just_pressed(key) then
            Sprite.set_label(totem, LOOKS[i])
            worn.top, worn.middle, worn.base = i, i, i
            show()
        end
    end
    for key, cat in pairs({ T = "top", M = "middle", G = "base" }) do
        if Input.is_key_just_pressed(key) then
            worn[cat] = worn[cat] % #LOOKS + 1
            Sprite.set_label(parts[cat], LOOKS[worn[cat]])
            show()
        end
    end
end
