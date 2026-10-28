-- Scene 298 - script maths, the same bits everywhere
--
-- A game whose rules live in Lua plays out the same on every machine only if
-- every number its script computes is the same there. Native runs Luau, the
-- editor and web builds run Fengari (Lua 5.3), and each used to compute
-- `math.sin`, `^`, `%` and `math.random` its own way. They all compute through
-- the engine's `script_math` now (docs/determinism-proposal.md, phase 3), and
-- this logs each answer exactly so `tools/conformance.mjs` can diff them:
--
--   node tools/conformance.mjs --scene scenes/298-script-maths.scene
--
-- A number is logged as its integer mantissa and power of two (`exact`),
-- worked out with arithmetic both dialects do exactly. Numbers' text is
-- probed too, since Fengari prints as Luau does (`patch-fengari.mjs`): the
-- same digits, `3` rather than `3.0`, and whole numbers past 2^31 that no
-- longer wrap.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/298-script-maths.scene"

local results = {}
local done = false

-- `v` as `m p e` with `v == m * 2^e` and `m` a whole number of 53 bits: halving
-- and doubling are exact, so this is the number to the last bit.
local function exact(v)
    if v ~= v then return "nan" end
    if v == math.huge then return "inf" end
    if v == -math.huge then return "-inf" end
    if v == 0 then return (1 / v < 0) and "-0" or "0" end
    -- A float first: a Lua 5.3 integer (Fengari's `math.random(6)`) doubles
    -- in 32 bits and wraps, so the loop below would never end.
    v = v + 0.0
    local sign = ""
    if v < 0 then sign = "-"; v = -v end
    local e = 0
    while v >= 9007199254740992 do v = v / 2; e = e + 1 end
    while v < 4503599627370496 do v = v * 2; e = e - 1 end
    return sign .. string.format("%.0f", v) .. "p" .. tostring(e)
end

local function probe(name, v)
    local text = type(v) == "number" and exact(v) or tostring(v)
    results[#results + 1] = name .. " = " .. text
    log("[conformance] " .. name .. " = " .. text)
end

-- Inputs held in a table, so neither compiler folds what is computed from them.
local IN = { 0.3, 1.234, -2.5, 7.75, 1e-3, 123.456 }

local function run()
    for i, x in ipairs(IN) do
        probe("sin " .. i, math.sin(x))
        probe("cos " .. i, math.cos(x))
        probe("tan " .. i, math.tan(x))
        probe("atan " .. i, math.atan(x))
        probe("atan2 " .. i, math.atan2(x, 0.7))
        probe("atan(y,x) " .. i, math.atan(x, -0.7))
        probe("exp " .. i, math.exp(x / 10))
        probe("sinh " .. i, math.sinh(x / 10))
        probe("cosh " .. i, math.cosh(x / 10))
        probe("tanh " .. i, math.tanh(x))
        local p = math.abs(x)
        probe("log " .. i, math.log(p))
        probe("log10 " .. i, math.log10(p))
        probe("log(x,10) " .. i, math.log(p, 10))
        probe("log(x,3) " .. i, math.log(p, 3))
        probe("pow " .. i, math.pow(p, 1.37))
        probe("^ var " .. i, p ^ (x / 3))
        probe("^ 0.5 " .. i, p ^ 0.5)
        probe("^ 3 " .. i, x ^ 3)
        probe("% " .. i, x % 0.1)
        probe("% neg " .. i, x % -0.7)
        probe("// " .. i, x // 0.3)
        probe("asin " .. i, math.asin(x / 200))
        probe("acos " .. i, math.acos(x / 200))
        probe("lerp " .. i, Math.lerp(x, 10.1, 0.37))
        probe("distance " .. i, Math.distance(x, 0.2, 3.3, x / 7))
        probe("angle_between " .. i, Math.angle_between(0.1, x, 2.2, 0.5))
    end
    -- Literals, which Luau's compiler folds at compile time and Fengari
    -- computes at run time.
    probe("literal sin", math.sin(1.234))
    probe("literal ^", 1.7 ^ 2.3)
    probe("literal %", 5.3 % 0.1)
    probe("literal exp", math.exp(1.7))
    probe("literal atan2", math.atan2(-1.5, 0.25))
    -- The one stream every script draws from.
    math.randomseed(12345)
    for i = 1, 8 do probe("random " .. i, math.random()) end
    for i = 1, 8 do probe("random 1-6 " .. i, math.random(6)) end
    for i = 1, 4 do probe("random 10-99 " .. i, math.random(10, 99)) end
    math.randomseed(12345)
    probe("reseeded", math.random())
    -- Whole numbers past 2^31, which Fengari's 32-bit integers wrapped.
    local big, small = #IN * 0 + 2000000000, 2147483647
    probe("whole + past 2^31", big + big)
    probe("whole * past 2^31", 65536 * (small // 32768 + 1))
    probe("whole - past -2^31", -small - 2)
    probe("whole //", (big + big) // 7)
    local count = 0
    for _ = 1, 3 do count = count + small end
    probe("whole counter", count)
    -- Text: `tostring`, `..` and `%d` of whole, fractional, large and tiny
    -- numbers.
    local texts = { 3, -0.0, 0.1, 1 / 3, IN[2] * 1e21, IN[5] / 1e4, big + big, 1e15, 2 ^ 53, -IN[4] }
    for i, v in ipairs(texts) do
        -- `%d` past 2^53 is each formatter's own (past 2^63, nonsense in both).
        local d = math.abs(v) < 9007199254740992 and string.format("%d", v) or "-"
        probe("text " .. i, "'" .. tostring(v) .. "' '" .. v .. "' '" .. d .. "'")
    end
end

function init(entity)
    results = {}
    done = false
end

function update(entity, dt)
    if not done then
        done = true
        run()
        log("[conformance] done " .. #results)
        Text.set(Entity.find_by_name("Summary"), tostring(#results) .. " numbers - compare runtimes with tools/conformance.mjs")
        local shown = {}
        for i = 1, math.min(#results, 24) do shown[i] = results[i] end
        Text.set(Entity.find_by_name("Lines"), table.concat(shown, "\n"))
    end
    nav.tick(SCENE_PATH)
end
