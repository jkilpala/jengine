-- Scene 175: fades the three figures. The left one fades each part (the old
-- way), the others are canvas groups faded with CanvasGroup.set.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/175-canvas-groups.scene"
local PARTS = {
    Body = { 0.35, 0.65, 0.95 }, Head = { 0.95, 0.8, 0.6 }, ArmL = { 0.25, 0.5, 0.85 },
    ArmR = { 0.25, 0.5, 0.85 }, LegL = { 0.2, 0.25, 0.4 }, LegR = { 0.2, 0.25, 0.4 },
}
local loose = {}
local group, tinted, status
local t, paused = 0, false

function init(entity)
    for name, c in pairs(PARTS) do
        loose[#loose + 1] = { e = Entity.find_by_name("Loose" .. name), c = c }
    end
    group = Entity.find_by_name("Group")
    tinted = Entity.find_by_name("Tinted")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then paused = not paused end
    if not paused then t = t + dt end
    local a = 0.2 + 0.8 * (0.5 + 0.5 * math.sin(t * 1.2))
    for _, p in ipairs(loose) do
        if p.e then Sprite.set_color(p.e, p.c[1], p.c[2], p.c[3], a) end
    end
    if group then CanvasGroup.set(group, "opacity", a) end
    if tinted then CanvasGroup.set(tinted, "opacity", a) end
    if status then UI.set_text(status, "opacity " .. math.floor(a * 100 + 0.5) .. "%") end
end
