-- Scene 65 - Custom bitmap fonts
--
-- `TextComponent.font_id` names a `.font` asset; empty means the built-in
-- Noto Sans. A `.font` is two files -- the JSON descriptor and the atlas image
-- beside it with the same stem (`fonts/x.font` -> `fonts/x.png`) -- and the
-- engine pairs them itself. Nothing in this scene loads anything: the runtimes
-- scan the world for referenced `font_id`s the same way they scan it for
-- `.gui` documents, so authoring the id is the whole job.
--
-- What each block proves, and what failure looks like:
--
--   A/B pair    Same string, same scale, different `font_id`. If the two lines
--               look identical, `font_id` is being ignored and everything is
--               drawing from the built-in atlas.
--   Accents     `x_offset` / `y_offset` from the descriptor. If descenders sit
--               on the baseline, those per-glyph fields were dropped -- which
--               is exactly what the editor's font tool used to do on save.
--   Fallback    A `font_id` naming a file that does not exist must fall back to
--               the built-in font. Blank text there means a missing font eats
--               the string, which is far harder to diagnose than a wrong face.
--   z_order     A custom-font line at z=5 over a built-in line at z=0. The
--               renderer groups text into *runs* sharing an atlas rather than
--               bucketing by font; if it bucketed, the yellow line would fall
--               behind the blue one purely because of which font it uses.
--   Ticker      `set_text` into a custom-font entity, so re-layout uses the
--               custom advances every frame.
--   Wrap        `wrap_width` measured with the custom font's own metrics.
--
-- Until Sep 2026 none of this worked: `.font` files could be authored in the
-- editor but nothing anywhere read one off disk, and `font_id` was declared,
-- serialized, and never looked at by any renderer.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/65-custom-fonts.scene"

local CUSTOM_FONT = "fonts/bebasneue-regular.font"

local ids = {}
local elapsed = 0.0

function init(entity)
    ids.readout = Entity.find_by_name("Readout")
    ids.ticker = Entity.find_by_name("Ticker")

    if ids.readout ~= nil then
        Text.set(
            ids.readout,
            "font: " .. CUSTOM_FONT .. "\n"
                .. "atlas: fonts/bebasneue-regular.png\n"
                .. "319 glyphs, 512x128, pixel_size 16\n\n"
                .. "The atlas is paired by stem, not named\n"
                .. "in the .font file."
        )
    end
    log("[scene_65] ready - built-in and " .. CUSTOM_FONT .. " side by side")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)

    elapsed = elapsed + dt
    if ids.ticker ~= nil then
        -- Re-laid out every frame with the custom font's advances. A string
        -- that jitters in width per digit is the advance table being read
        -- from the wrong font.
        Text.set(ids.ticker, string.format("ELAPSED %.2f S", elapsed))
    end
end
