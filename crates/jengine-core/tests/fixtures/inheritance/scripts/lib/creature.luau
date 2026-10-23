-- lib/creature.luau — root of the inheritance chain.
--
-- Library scripts are loaded via `require()` (or implicitly by an attached
-- script that declares `extends = "lib/creature"`) and must return a table
-- of fields + functions. Any field not overridden by a child is found via
-- the inheritance metatable chain.

local Creature = {}

Creature.species   = "creature"
Creature.max_hp    = 100
Creature.tick_count = 0

function Creature:greet()
    log(string.format("[creature] hello — I am a %s", self.species))
end

function Creature:attack()
    log(string.format("[creature] %s swings wildly", self.species))
end

-- The base update prints a heartbeat every 60 ticks. Child scripts inherit
-- it unless they define their own `update`. It has to be a field of the
-- returned table: a plain `function update` here would be a global of this
-- module alone, and nothing that extends it would see it.
function Creature.update(entity, dt)
    Creature.tick_count = Creature.tick_count + 1
    if Creature.tick_count % 60 == 0 then
        log(string.format("[creature] tick %d (dt=%.4f)", Creature.tick_count, dt))
    end
end

return Creature
