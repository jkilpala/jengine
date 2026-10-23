-- Scene 186: fonts from .ttf files. Each line's font_id names a font file;
-- its glyphs are baked the first time a text asks for them, so changing a
-- text to new characters just works.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/186-font-files.scene"
local A = { "Привет, мир", "Γειά σου κόσμε", "こんにちは世界", "你好，世界", "안녕하세요 세계" }
local B = { "Самовар, Базар", "Ταβέρνα, Αγορά", "茶屋 ラーメン", "丝绸之路 茶馆", "찻집 시장" }
local which = false
local lines = {}

function init(entity)
    for i = 0, 4 do lines[#lines + 1] = Entity.find_by_name("Line" .. i) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("T") then
        which = not which
        for i, e in ipairs(lines) do
            if e then Text.set(e, which and B[i] or A[i]) end
        end
    end
end
