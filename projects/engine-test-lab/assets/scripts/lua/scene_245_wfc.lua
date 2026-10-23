-- Scene 245: `wfc`. The map's generator fills the layer at 60 cells a
-- second; Space asks for the whole map again, 1 and 2 for one half.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/245-wfc.scene"
local COLS, ROWS = 24, 12
local map, line
local maps = 0

function init(entity)
    map = entity
    line = Entity.find_by_name("Line0")
    Wfc.generate(map, 0, 0, COLS, ROWS)
end

function on_wfc_done(entity)
    maps = maps + 1
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then Wfc.generate(map, 0, 0, COLS, ROWS) end
    if Input.is_key_just_pressed("1") then Wfc.generate(map, 0, 0, COLS / 2, ROWS) end
    if Input.is_key_just_pressed("2") then Wfc.generate(map, COLS / 2, 0, COLS / 2, ROWS) end
    UI.set_text(line, "regions finished " .. maps)
end
