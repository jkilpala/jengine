-- Scene 164: the lines with a `text_key` are the engine's (`locale`, over the
-- string table `locale/lab.data`); this switches the language and writes the
-- two lines that carry numbers through Locale.get.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/164-localization.scene"
local CODES = { "en", "fi", "de", "pl" }
local rooms_label, price_label, readout
local rooms = 5

function init(entity)
    rooms_label = Entity.find_by_name("Rooms")
    price_label = Entity.find_by_name("Price")
    readout = Entity.find_by_name("Readout")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    for i, code in ipairs(CODES) do
        if Input.is_key_just_pressed(tostring(i)) then Locale.set(code) end
    end
    if Input.is_key_just_pressed("0") then Locale.set("") end
    if Input.is_key_just_pressed("Up") then rooms = rooms + 1 end
    if Input.is_key_just_pressed("Down") and rooms > 0 then rooms = rooms - 1 end
    Text.set(rooms_label, Locale.get("lab_rooms", { n = rooms }))
    Text.set(price_label, Locale.get("lab_price", { p = 3.5 }))
    local names = {}
    for _, code in ipairs(Locale.list()) do names[#names + 1] = code .. " " .. Locale.name(code) end
    Text.set(readout, "Locale.current() = " .. Locale.current() .. "    Locale.list(): " .. table.concat(names, ", "))
end
