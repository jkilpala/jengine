-- lib/npc.luau — middle layer. Inherits everything from `lib/creature` and
-- adds dialogue. Demonstrates a 2-level inheritance chain.

script = { extends = "lib/creature" }

local NPC = {}

NPC.species = "npc"
NPC.dialogue = "Greetings, traveller."

function NPC:speak()
    log(string.format("[npc] %s says: %s", self.species, self.dialogue))
end

return NPC
