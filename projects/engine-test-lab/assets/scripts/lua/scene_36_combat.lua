-- Scene 36 - Combat (Health / Hitbox / HealthBar / DespawnOnDeath)
--
-- The damage loop is entirely engine-side: `apply_hitbox_damage` reads each
-- Hitbox's TriggerOverlaps (so a hitbox needs a TriggerArea2D to see anyone),
-- subtracts damage from the victim's Health, respects the victim's i-frames
-- and the attacker's retrigger cooldown, then DespawnOnDeath removes whatever
-- reaches zero. HealthBar draws the bar.
--
-- Nothing below is required for that to work - the script only reports the
-- state and provides heal / damage / reset keys so the rules are easy to see.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/36-combat.scene"

local DUMMIES = { "Dummy1", "Dummy2", "Dummy3" }
local MAX_HP = 100

local readout
local buffed = false

local function each_dummy(fn)
    for _, name in ipairs(DUMMIES) do
        local e = Entity.find_by_name(name)
        if e ~= nil then
            fn(e, name)
        end
    end
end

function init(entity)
    readout = Entity.find_by_name("Readout")
    log("[scene_36] ready - Space heals, K kills #2, B buffs the saw hitbox, R resets")
end

function update(entity, dt)
    if Input.is_key_just_pressed("Space") then
        each_dummy(function(e) Entity.set_health(e, MAX_HP, MAX_HP) end)
        log("[scene_36] healed all")
    elseif Input.is_key_just_pressed("K") then
        local e = Entity.find_by_name("Dummy2")
        -- Zero HP plus DespawnOnDeath means the entity is gone next frame.
        if e ~= nil then Entity.set_health(e, 0, MAX_HP) end
    elseif Input.is_key_just_pressed("B") then
        -- set_hitbox rewrites damage / knockback / retrigger on a live
        -- hitbox; `active` and the cooldown table are preserved.
        local saw = Entity.find_by_name("Saw")
        if saw ~= nil then
            buffed = not buffed
            if buffed then
                Entity.set_hitbox(saw, 40.0, 0.0, 0.15)
            else
                Entity.set_hitbox(saw, 12.0, 0.0, 0.5)
            end
            log("[scene_36] saw hitbox " .. (buffed and "buffed (40 dmg, 0.15s)" or "default (12 dmg, 0.5s)"))
        end
    elseif Input.is_key_just_pressed("R") then
        Scene.load(SCENE_PATH)
        return
    end

    if readout then
        -- Entity.get_health(entity) -> current, max. It returns nil for an entity
        -- with no Health, which is how "no component" stays distinguishable
        -- from "dead". Note the engine-drawn HealthBar above each dummy
        -- should agree with these numbers exactly.
        local parts = {}
        local alive = 0
        for _, name in ipairs(DUMMIES) do
            local e = Entity.find_by_name(name)
            if e == nil then
                parts[#parts + 1] = name .. ": dead"
            else
                alive = alive + 1
                local hp, max_hp = Entity.get_health(e)
                parts[#parts + 1] = string.format("%s: %d/%d", name,
                    math.floor((hp or 0) + 0.5), math.floor((max_hp or 0) + 0.5))
            end
        end
        Text.set(readout, table.concat(parts, "   |   ")
            .. string.format("   (%d/3 alive)", alive))
    end

    nav.tick(SCENE_PATH)
end
