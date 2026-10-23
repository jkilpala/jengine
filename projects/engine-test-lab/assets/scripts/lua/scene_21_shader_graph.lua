-- Scene 21: Shader Graph (Pulse)
-- First end-to-end demo of the visual shader editor pipeline
-- (Slice 13). The Pulsing sprite carries a MaterialComponent whose
-- material_id resolves to assets/materials/pulse_emission.material,
-- which references assets/shaders/pulse_emission.shader. At first
-- flush the runtime lazy-loads the material, runs the graph through
-- `shader_codegen::generate`, compiles a wgpu pipeline via
-- `GraphPipelineCache`, and uses `flush_with_pipeline` for the
-- material-tagged batch. Time uniform comes from the Globals UBO
-- (sprite_batch.set_time) so the sin() oscillates without script.

-- The three Rim sprites are the second half, added Sep 2026 with
-- `NodeType::VertexColorInput`. They share one material and one texture and
-- differ **only** in their sprite colour, which the script ramps below - so
-- what you are watching is a graph depending on something the game decided.
--
-- That was impossible before. A graph could read UV, time, position, the
-- sampled texture and constants, and nothing else; `MaterialComponent`
-- carries a `uniforms` map that four documented script functions write and no
-- render path reads (see docs/parity-gaps.md). The vertex colour was already
-- being interpolated into the fragment stage and multiplied by the default
-- sprite shader - exposing it as a node costs no binding and does not break
-- batching, which a real per-entity uniform would.
--
-- All three ramp through the whole range, a third of a cycle apart, so at any
-- moment one is below the threshold, one is crossing it and one is past. The
-- one crossing is what to watch: the rim should fade in through the
-- `smoothstep`, not snap on.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/21-shader-graph.scene"

local rims = {}
local tints = {}
local t = 0

function init(entity)
    for i = 1, 3 do
        rims[i] = Entity.find_by_name("Rim" .. i)
        tints[i] = Entity.find_by_name("Tint" .. i)
    end
    log("[21-shader-graph] ready - pulsing sprite uses graph-compiled material, "
        .. "three swatches drive one graph from their vertex colour")
end

function update(entity, dt)
    t = t + dt
    -- A slow ramp per swatch, offset so the three are never in the same place
    -- on the curve at once.
    for i = 1, 3 do
        if rims[i] ~= nil then
            local phase = t * 0.5 + (i - 1) * 0.33
            local k = 0.25 + 0.7 * (0.5 + 0.5 * math.sin(phase * math.pi * 2))
            Sprite.set_color(rims[i], k, k * 0.92, k * 0.7, 1.0)
        end
    end
    -- The three Tint swatches share `param_tint` and one texture too, and
    -- differ only in the `tint` parameter set here - a number the graph
    -- reads through a `MaterialParam` node (Sep 2026). Before that this call
    -- wrote `MaterialComponent.uniforms`, which nothing read: the swatches
    -- would all have drawn black. Each cycles through the hues a third of a
    -- turn apart.
    for i = 1, 3 do
        if tints[i] ~= nil then
            local h = t * 0.2 + (i - 1) / 3
            local r = 0.5 + 0.5 * math.sin(h * math.pi * 2)
            local g = 0.5 + 0.5 * math.sin((h + 1 / 3) * math.pi * 2)
            local b = 0.5 + 0.5 * math.sin((h + 2 / 3) * math.pi * 2)
            Material.set_vec4(tints[i], "tint", r, g, b, 1.0)
        end
    end
    nav.tick(SCENE_PATH)
end
