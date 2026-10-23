-- goblin.luau — leaf script attached to the Goblin entity in default.scene.
--
-- Inherits from `lib/npc` which itself inherits from `lib/creature`. Because
-- this file does NOT define its own `update`, the engine walks the metatable
-- chain and ends up calling Creature's `update` from `lib/creature.luau` —
-- which is exactly what hot-reload should toggle when you edit that file at
-- runtime.

script = { extends = "lib/npc" }

-- Override `attack` to demonstrate a leaf-level override.
function attack(entity)
    log("[goblin] *cackles* and lunges at the player!")
end

-- Called once when the entity is spawned.
function init(entity)
    log("[goblin] spawned — inheritance chain: goblin → lib/npc → lib/creature")
    if super and super.greet then
        super:greet()
    end
end
