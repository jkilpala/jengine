-- Scene 286: `nav_link`, `NavLink.set`, the `NavCrossing` fields and
-- `on_nav_link`.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/286-nav-links.scene"
local line, runner, jump, chute
local jump_on, chute_on = true, true
local crossings = 0

function init(entity)
    line = Entity.find_by_name("Line0")
    runner = Entity.find_by_name("Runner")
    jump = Entity.find_by_name("Jump")
    chute = Entity.find_by_name("Chute")
    Nav.set_target(runner, 450, 0)
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_mouse_left_just_pressed() then
        local mx, my = Input.get_mouse_world_position()
        Nav.set_target(runner, mx, my)
    end
    if Input.is_key_just_pressed("J") then
        jump_on = not jump_on
        NavLink.set(jump, "enabled", jump_on)
        Sprite.set_color(jump, 0.95, 0.8, 0.3, jump_on and 1 or 0.2)
    end
    if Input.is_key_just_pressed("C") then
        chute_on = not chute_on
        NavLink.set(chute, "enabled", chute_on)
        Sprite.set_color(chute, 0.4, 0.85, 1.0, chute_on and 1 or 0.2)
    end
    local kind = Entity.get_custom(runner, "NavCrossing", "kind") or ""
    local t = Entity.get_custom(runner, "NavCrossing", "t") or 0
    local lift = kind == "jump" and math.sin(math.pi * t) or 0
    Transform.set_scale(runner, 1 + 0.6 * lift, 1 + 0.6 * lift)
    if kind ~= "" then
        UI.set_text(line, "crossing: " .. kind .. " " .. math.floor(t * 100 + 0.5) .. "%")
    else
        UI.set_text(line, (Nav.is_moving(runner) and "running" or "waiting")
            .. "   jump " .. (jump_on and "on" or "off") .. "   chute " .. (chute_on and "on" or "off"))
    end
end
