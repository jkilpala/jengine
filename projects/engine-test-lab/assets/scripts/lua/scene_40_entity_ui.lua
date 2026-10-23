-- Scene 40 - Entity UI
--
-- jEngine has two UI systems and this is the other one. Scene 10 covers `.gui`
-- documents (a whole tree in one asset, laid out by ui::layout). Here every
-- widget is its own entity: a UIElement for placement (anchor + offset + size +
-- z_order) plus exactly one widget component (UIPanel / UIText / UIImage /
-- UIButton / UIProgressBar / UISlider / UIToggle / UITextInput).
--
-- They render through a different path (render_ui_wgpu) and are reached from
-- scripts with a different API, so a bug in one system says nothing about the
-- other.
--
-- Every widget below can be driven from script via `UI.set_prop(entity,
-- property, value)`. One function covers all of them because the widget fields
-- have no common type; the property name picks the field, and a property that
-- no widget on the entity has is a logged no-op rather than an error.
--
-- Since Sep 2026 they also answer the **pointer**: drag the slider, click the
-- toggle, click the field and type. Before that only the script could move
-- them - `update_ui_buttons` was never called by any runtime and the other
-- three widgets had no interaction code at all (BUGS #61). The readout at the
-- bottom reads every value back with `get_ui_prop`, so what it shows is what
-- the widgets actually hold, not what the script last wrote.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/40-entity-ui.scene"

local t = 0.0
local panel_visible = true
local checked = false
local clicks = 0
local hovered = false
local disabled = false
local made_label = nil
local made_panel = nil
local readout, bar, slider, toggle, input, button, bar_label, region_icon
-- A `ui_image` names an atlas region like a sprite (Sep 2026); this one
-- walks through the monster sheet, swapped with `UI.set_prop(e, "region")`.
local MONSTERS = { "embit", "puddle", "sprout", "nibble", "pidge", "mossbug", "cinder", "tidal" }
local monster = 1
local monster_t = 0.0

function init(entity)
    readout = Entity.find_by_name("Readout")
    bar = Entity.find_by_name("Bar")
    bar_label = Entity.find_by_name("BarLabel")
    slider = Entity.find_by_name("Slider")
    toggle = Entity.find_by_name("Toggle")
    input = Entity.find_by_name("Input")
    button = Entity.find_by_name("Button")
    region_icon = Entity.find_by_name("RegionIcon")
    -- A click is an event, delivered on release over the button it began on.
    Event.on("ui_click", entity)

    -- Widgets can also be created from script rather than authored: this one
    -- exists nowhere in the .scene file.
    -- Sits in the band between the authored panel (which ends 510px down) and
    -- the readout line at the bottom, so nothing is drawn over anything.
    made_panel = UI.create_panel("bottom_left", 28, 80, 300, 60)
    made_label = UI.create_text("created from script ->", "bottom_left", 40, 95)
    log("[scene_40] ready - drag the slider, click the toggle, type in the field")
    log("[scene_40] Space hides/shows the panel, T toggles from script, B disables the button")
end

function update(entity, dt)
    t = t + dt

    -- set_ui_position re-anchors a live widget every frame.
    if made_label ~= nil then
        local ox = 40 + math.sin(t) * 60
        -- A shallow swing: it has to stay inside the panel it moves across and
        -- off the readout underneath.
        local oy = 95 + math.cos(t) * 15
        UI.set_position(made_label, "bottom_left", ox, oy)
        UI.set_text(made_label, string.format("script label  t=%.1f", t))
    end

    -- The progress bar is script-driven; the slider is *yours*. Writing the
    -- slider every frame here would fight the drag - the engine sets the value
    -- from the pointer and the next tick would snap it back - so a widget the
    -- player operates and a widget the game animates must not be the same
    -- widget. The bar's range is 0..100 and the slider's 0..1, and set_ui_prop
    -- clamps to each widget's own min/max - which is why the bar's range in
    -- the .scene has to be the range this writes. It said 0..1 for a while, so
    -- every value above 1 clamped to full and the bar sat pinned at 100% while
    -- the label beside it counted up and down. The readout at the bottom reads
    -- the bar back with `get_ui_prop`, so the two disagreeing is the tell.
    monster_t = monster_t + dt
    if monster_t >= 1.0 and region_icon ~= nil then
        monster_t = 0.0
        monster = monster % #MONSTERS + 1
        UI.set_prop(region_icon, "region", MONSTERS[monster])
    end

    local wave = (math.sin(t) + 1) * 0.5
    UI.set_prop(bar, "value", wave * 100)
    UI.set_prop(bar_label, "text", string.format("bar %.0f%%", wave * 100))

    -- Colour is an {r,g,b,a} table; the bar turns red as it fills.
    UI.set_prop(bar_label, "color", { 1.0, 1.0 - wave, 1.0 - wave, 1.0 })

    if Input.is_key_just_pressed("T") then
        checked = not checked
        UI.set_prop(toggle, "checked", checked)
        -- The text input is writable too, which is how a script pre-fills a
        -- field or clears one after a submit.
        UI.set_prop(input, "text", checked and "toggled on" or "")
    end

    -- Clicks arrive in `on_ui_click` below. `pressed` / `hovered` are still
    -- maintained every frame, for drawing: `pressed` is true for every frame
    -- the button is held, so it is not a click.
    hovered = UI.get_prop(button, "hovered") == true

    if Input.is_key_just_pressed("B") then
        -- `disabled` greys the button out and stops it taking hover/press.
        disabled = not disabled
        UI.set_prop(button, "disabled", disabled)
    end

    if Input.is_key_just_pressed("Space") then
        panel_visible = not panel_visible
        for _, name in ipairs({ "Panel", "PanelTitle", "Thumb", "Caption", "RegionIcon" }) do
            local e = Entity.find_by_name(name)
            if e ~= nil then
                UI.set_visible(e, panel_visible)
            end
        end
    end

    if readout then
        -- Every number here was read back with get_ui_prop, not remembered by
        -- the script: the widgets are the source of truth.
        Text.set(readout, string.format(
            "panel %s   bar %.0f%%   slider %.2f   toggle %s   typed %q   button %s%s   clicks %d",
            panel_visible and "visible" or "hidden",
            (UI.get_prop(bar, "value") or 0),
            (UI.get_prop(slider, "value") or 0),
            (UI.get_prop(toggle, "checked") == true) and "on" or "off",
            (UI.get_prop(input, "text") or ""),
            hovered and "hover" or "idle",
            disabled and " (disabled)" or "",
            clicks))
    end

    nav.tick(SCENE_PATH)
end

-- The entity-UI answer to `gui_on_click`: every runtime raises `ui_click`
-- with the button's id. Before Sep 2026 each runtime collected the clicks
-- and dropped them, and this scene polled `pressed` for its edge instead.
function on_ui_click(entity, data)
    if data ~= nil and data.button == button then
        clicks = clicks + 1
        log("[scene_40] button clicked " .. clicks)
    end
end
