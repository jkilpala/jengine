-- Scene 11 - World-space TextComponent showcase.
--
-- Demonstrates the TextComponent bitmap-font renderer:
--   * Static labels at different font_scale values (authored in .scene)
--   * Static colour swatches (authored in .scene)
--   * z_order layering: ZBack (z=0, blue) and ZFront (z=5, yellow) overlap;
--     pressing R/F flips which one wins
--   * Live set_text mutation: frame counter, elapsed seconds, typewriter
--   * Live set_position mutation: a Floater label orbits a fixed point
--   * Live set_scale mutation: Title pulses gently
--
-- Controls:
--   SPACE     - restart the typewriter from char 0
--   R         - reset frame counter
--   F         - swap z_order of the two overlapping labels
--   H / Esc   - back to hub
--   [ / ]     - prev / next scene in tour

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/11-text.scene"

local TYPE_TARGET = "type, set_text, render"
local TYPE_SPEED  = 14.0  -- chars per second

local t = 0.0
local frames = 0
local frames_offset = 0
local typewriter_t = 0.0

local prev_space  = false
local prev_r      = false
local prev_f      = false

local front_is_yellow = true

-- Cached entity ids (resolved on first update so the spawn pass has finished).
local ids = nil
local function resolve_ids()
    ids = {
        title      = Entity.find_by_name("Title"),
        counter    = Entity.find_by_name("Counter"),
        clock      = Entity.find_by_name("Clock"),
        typewriter = Entity.find_by_name("Typewriter"),
        floater    = Entity.find_by_name("Floater"),
        z_back     = Entity.find_by_name("ZBack"),
        z_front    = Entity.find_by_name("ZFront"),
    }
end

function init(entity)
    log("[scene_11] ready - SPACE restart typewriter, R reset counter, F flip z-order")
end

function update(entity, dt)
    if not ids then resolve_ids() end

    t = t + dt
    frames = frames + 1

    -- Edge-triggered keys.
    local space_now = Input.is_key_pressed("Space")
    local r_now     = Input.is_key_pressed("R")
    local f_now     = Input.is_key_pressed("F")
    local space_edge = space_now and not prev_space
    local r_edge     = r_now     and not prev_r
    local f_edge     = f_now     and not prev_f
    prev_space, prev_r, prev_f = space_now, r_now, f_now

    if space_edge then
        typewriter_t = 0.0
        if ids.typewriter and ids.typewriter ~= 0 then
            Text.set(ids.typewriter, "")
        end
    end
    if r_edge then
        frames_offset = frames
    end
    if f_edge then
        front_is_yellow = not front_is_yellow
        -- Swap which label is visually on top by toggling the text on
        -- each: empty string makes one invisible, full text makes the
        -- other visible. (TextComponent has no `visible` field, so this
        -- is the canonical hide trick.)
        if ids.z_back and ids.z_front and ids.z_back ~= 0 and ids.z_front ~= 0 then
            if front_is_yellow then
                Text.set(ids.z_back,  "BACK z=0 BACK z=0 BACK z=0")
                Text.set(ids.z_front, "  FRONT z=5  ")
            else
                Text.set(ids.z_back,  "BACK z=0 BACK z=0 BACK z=0")
                Text.set(ids.z_front, "")
            end
        end
    end

    -- Live counter + clock via set_text.
    if ids.counter and ids.counter ~= 0 then
        Text.set(ids.counter, string.format("frame: %8d", frames - frames_offset))
    end
    if ids.clock and ids.clock ~= 0 then
        Text.set(ids.clock, string.format("elapsed: %6.2f s", t))
    end

    -- Typewriter: append one character at a time until the target string is
    -- fully revealed, then idle.
    if ids.typewriter and ids.typewriter ~= 0 then
        typewriter_t = typewriter_t + dt
        local n = math.floor(typewriter_t * TYPE_SPEED)
        if n > #TYPE_TARGET then n = #TYPE_TARGET end
        Text.set(ids.typewriter, string.sub(TYPE_TARGET, 1, n) .. "_")
    end

    -- Floater orbits around (300, -240) with a 80-px radius.
    if ids.floater and ids.floater ~= 0 then
        local cx, cy = 300.0, -240.0
        local rad = 80.0
        local angle = t * 1.2
        Transform.set_position(ids.floater, cx + math.cos(angle) * rad, cy + math.sin(angle) * rad)
    end

    -- Title pulses scale between 0.9 and 1.15.
    if ids.title and ids.title ~= 0 then
        local s = 1.0 + math.sin(t * 2.5) * 0.12
        Transform.set_scale(ids.title, s, s)
    end

    nav.tick(SCENE_PATH)
end
