-- 154 Talk of the Town: everyone here has a `speech_bubble`. The script
-- only says who says what - `Bubble.say(e, text, seconds)` - and the
-- engine sizes each bubble to its words, points its tail down at the
-- speaker, keeps it on screen and pushes it clear of the others.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/154-talk-of-the-town.scene"
local LINES = {
    Greengrocer = { "Apples! Crisp as frost!", "Turnips, three for a penny.", "Mind the cabbages, they bruise." },
    Baker = { "Fresh bread, warm from the oven, two loaves for a copper!", "Honey buns!", "Crusts for the pigeons, free." },
    Fishwife = { "Herring, caught this morning!", "Eels! Lively eels!", "You'll not find fresher this side of the river." },
    Cheesemonger = { "A wheel of the blue? It's ripe, mind.", "Try a sliver, go on." },
    Florist = { "Roses for your sweetheart?", "Daisies, a farthing a bunch." },
    Traveller = { "Which way to the inn?", "What a din!", "Is that bread I smell?" },
    Child = { "Can I have a bun?", "Look, a cat!", "Mum! Mum! Mum!" },
    Guard = { "Move along now.", "No haggling in the road.", "All quiet?" },
}
local WALKERS = { { "Traveller", 1, 125.0 }, { "Child", -1, 140.0 }, { "Guard", 1, 155.0 } }
local status
local captions = true
local folk, names = {}, {}
local walkers = {}
local t, next_in = 0, 0.6

local function speak(name)
    local lines = LINES[name]
    local line = lines[math.random(#lines)]
    Bubble.say(folk[name], line)
    UI.set_text(status, name .. " says something")
end

function init(entity)
    status = Entity.find_by_name("Status")
    for name, _ in pairs(LINES) do
        folk[name] = Entity.find_by_name(name)
        table.insert(names, name)
    end
    table.sort(names)
    for _, w in ipairs(WALKERS) do
        local e = folk[w[1]]
        local x, y = Transform.get_position(e)
        table.insert(walkers, { e = e, x = x, y = y, dir = w[2], speed = w[3] })
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    next_in = next_in - dt
    if next_in <= 0 then
        speak(names[math.random(#names)])
        next_in = 0.35 + math.random() * 0.6
    end
    if Input.is_mouse_left_just_pressed() then
        local mx, my = Input.get_mouse_world_position()
        for _, name in ipairs(names) do
            local x, y = Transform.get_position(folk[name])
            if math.abs(mx - x) < 35 and math.abs(my - y) < 70 then speak(name) end
        end
    end
    for _, w in ipairs(walkers) do
        w.x = w.x + w.dir * w.speed * dt
        if w.x > 700 then w.dir = -1 elseif w.x < -700 then w.dir = 1 end
        Transform.set_position(w.e, w.x, w.y + math.abs(math.sin(t * 6 + w.speed)) * 4)
        Transform.set_scale(w.e, w.dir, 1)
    end
end
