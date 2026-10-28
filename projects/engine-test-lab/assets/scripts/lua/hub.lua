-- Hub menu script for the Engine Test Lab.
--
-- Owns the click bindings for `ui/hub.gui`. Adding a scene means: a row in
-- `BUTTONS` below, a matching Button node under `scene_list` in the .gui
-- document, and an entry in `nav.lua`'s TOUR — no other wiring needed.
--
-- The button list lives in an `overflow: "Scroll"` container, so it no longer
-- matters how long it gets: the engine clips it to the frame, takes the mouse
-- wheel, and draws the scrollbar. Up/Down, a D-pad or a stick walk the
-- buttons and the engine keeps the focused one in view; this script adds
-- page jumps (PageUp/PageDown, Home/End) via `GUI.set(..., "scroll_y", n)`.

local GUI_PATH = "ui/hub.gui"
local LIST_ID = "scene_list"
-- The frame's height is `Percent` in the .gui, not computed here. A scroll
-- container is clipped to its own box and nothing clips that box to the
-- screen, so a frame taller than the window hides its last rows somewhere
-- scrolling cannot reach. Percent lets the layout engine derive the height
-- from the screen size it is already using, which keeps the two in step
-- without this script knowing anything about the window.
--
local ROW_STEP = 54
local PAGE_STEP = ROW_STEP * 5

-- Ordered list of { button_id, scene_path } pairs. The hub is the only
-- screen where this routing table lives; per-scene navigation (next/prev/
-- back) is handled by `scripts/lua/nav.lua`.
local BUTTONS = {
    { "btn_scene_01", "scenes/01-transforms.scene" },
    { "btn_scene_02", "scenes/02-sprites.scene" },
    { "btn_scene_03", "scenes/03-camera.scene" },
    { "btn_scene_04", "scenes/04-input.scene" },
    { "btn_scene_05", "scenes/05-prefabs.scene" },
    { "btn_scene_06", "scenes/06-animations.scene" },
    { "btn_scene_07", "scenes/07-particles.scene" },
    { "btn_scene_08", "scenes/08-lighting.scene" },
    { "btn_scene_09", "scenes/09-tilemap.scene" },
    { "btn_scene_10", "scenes/10-gui.scene" },
    { "btn_scene_11", "scenes/11-text.scene" },
    { "btn_scene_12", "scenes/12-materials.scene" },
    { "btn_scene_13", "scenes/13-behavior.scene" },
    { "btn_scene_14", "scenes/14-post.scene" },
    { "btn_scene_15", "scenes/15-atlas.scene" },
    { "btn_scene_16", "scenes/16-audio.scene" },
    { "btn_scene_17", "scenes/17-physics.scene" },
    { "btn_scene_18", "scenes/18-collision-events.scene" },
    { "btn_scene_19", "scenes/19-scorekeeper.scene" },
    { "btn_scene_20", "scenes/20-pause-menu.scene" },
    { "btn_scene_21", "scenes/21-shader-graph.scene" },
    { "btn_scene_22", "scenes/22-vector-paths.scene" },
    { "btn_scene_23", "scenes/23-behaviors.scene" },
    { "btn_scene_24", "scenes/24-steering.scene" },
    { "btn_scene_25", "scenes/25-bounds-lifetime.scene" },
    { "btn_scene_26", "scenes/26-joints.scene" },
    { "btn_scene_27", "scenes/27-triggers.scene" },
    { "btn_scene_28", "scenes/28-spotlights.scene" },
    { "btn_scene_29", "scenes/29-collision-scripts.scene" },
    { "btn_scene_30", "scenes/30-sprite-fill.scene" },
    { "btn_scene_31", "scenes/31-shadows.scene" },
    { "btn_scene_32", "scenes/32-light-occlusion.scene" },
    { "btn_scene_33", "scenes/33-scroll-views.scene" },
    { "btn_scene_34", "scenes/34-skeleton-ik.scene" },
    { "btn_scene_35", "scenes/35-hierarchy.scene" },
    { "btn_scene_36", "scenes/36-combat.scene" },
    { "btn_scene_37", "scenes/37-custom-components.scene" },
    { "btn_scene_38", "scenes/38-save.scene" },
    { "btn_scene_39", "scenes/39-data-tables.scene" },
    { "btn_scene_40", "scenes/40-entity-ui.scene" },
    { "btn_scene_41", "scenes/41-tweens.scene" },
    { "btn_scene_42", "scenes/42-flocking.scene" },
    { "btn_scene_43", "scenes/43-profiling.scene" },
    { "btn_scene_44", "scenes/44-gamepad.scene" },
    { "btn_scene_45", "scenes/45-anim-state-machine.scene" },
    { "btn_scene_46", "scenes/46-sort-layers.scene" },
    { "btn_scene_47", "scenes/47-motion-components.scene" },
    { "btn_scene_48", "scenes/48-timers-events.scene" },
    { "btn_scene_49", "scenes/49-tween-api.scene" },
    { "btn_scene_50", "scenes/50-tags-queries.scene" },
    { "btn_scene_51", "scenes/51-tilemap-pathfinding.scene" },
    { "btn_scene_52", "scenes/52-debug-draw.scene" },
    { "btn_scene_53", "scenes/53-camera-fx.scene" },
    { "btn_scene_54", "scenes/54-input-rebinding.scene" },
    { "btn_scene_55", "scenes/55-audio-buses.scene" },
    { "btn_scene_56", "scenes/56-physics-forces.scene" },
    { "btn_scene_57", "scenes/57-particle-bursts.scene" },
    { "btn_scene_58", "scenes/58-batch-scripts.scene" },
    { "btn_scene_59", "scenes/59-packages.scene" },
    { "btn_scene_60", "scenes/60-project-packages.scene" },
    { "btn_scene_61", "scenes/61-swarm.scene" },
    { "btn_scene_62", "scenes/62-editor-stress.scene" },
    { "btn_scene_63", "scenes/63-spawned-physics.scene" },
    { "btn_scene_64", "scenes/64-patrol-paths.scene" },
    { "btn_scene_65", "scenes/65-custom-fonts.scene" },
    { "btn_scene_66", "scenes/66-networking.scene" },
    { "btn_scene_67", "scenes/67-ui-scaling.scene" },
    { "btn_scene_68", "scenes/68-graph-commands.scene" },
    { "btn_scene_81", "scenes/81-character.scene" },
    { "btn_scene_82", "scenes/82-camera-turn.scene" },
    { "btn_scene_83", "scenes/83-frame-events.scene" },
    { "btn_scene_84", "scenes/84-impulses.scene" },
    { "btn_scene_85", "scenes/85-despawn.scene" },
    { "btn_scene_86", "scenes/86-disabled.scene" },
    { "btn_scene_87", "scenes/87-prefab-textures.scene" },
    { "btn_scene_88", "scenes/88-ysort.scene" },
    { "btn_scene_89", "scenes/89-tile-zero.scene" },
    { "btn_scene_90", "scenes/90-ccd.scene" },
    { "btn_scene_91", "scenes/91-joint-anchors.scene" },
    { "btn_scene_92", "scenes/92-raycast.scene" },
    { "btn_scene_93", "scenes/93-frame-camera.scene" },
    { "btn_scene_94", "scenes/94-tilemap-chunks.scene" },
    { "btn_scene_95", "scenes/95-frame-pacing.scene" },
    { "btn_scene_96", "scenes/96-scene-churn.scene" },
    { "btn_scene_97", "scenes/97-occluder-budget.scene" },
    { "btn_scene_98", "scenes/98-searchlight.scene" },
    { "btn_scene_99", "scenes/99-textured-mesh.scene" },
    { "btn_scene_100", "scenes/100-graph-events.scene" },
    { "btn_scene_101", "scenes/101-wind-and-drag.scene" },
    { "btn_scene_102", "scenes/102-emit-shapes.scene" },
    { "btn_scene_103", "scenes/103-script-inheritance.scene" },
    { "btn_scene_104", "scenes/104-namespaces.scene" },
    { "btn_scene_105", "scenes/105-touch-controls.scene" },
    { "btn_scene_106", "scenes/106-safe-area.scene" },
    { "btn_scene_107", "scenes/107-controller-menus.scene" },
    { "btn_scene_108", "scenes/108-multi-touch.scene" },
    { "btn_scene_109", "scenes/109-hdr.scene" },
    { "btn_scene_110", "scenes/110-blend-and-particles.scene" },
    { "btn_scene_111", "scenes/111-unlit-sorting.scene" },
    { "btn_scene_112", "scenes/112-occluder-self-shadow.scene" },
    { "btn_scene_113", "scenes/113-shadow-length.scene" },
    { "btn_scene_114", "scenes/114-light-height.scene" },
    { "btn_scene_115", "scenes/115-noise-nodes.scene" },
    { "btn_scene_116", "scenes/116-scene-color.scene" },
    { "btn_scene_117", "scenes/117-sheared-children.scene" },
    { "btn_scene_118", "scenes/118-lights-and-decals.scene" },
    { "btn_scene_119", "scenes/119-lightmap-graphs.scene" },
    { "btn_scene_120", "scenes/120-many-lights.scene" },
    { "btn_scene_121", "scenes/121-ambient-occlusion.scene" },
    { "btn_scene_122", "scenes/122-trails.scene" },
    { "btn_scene_123", "scenes/123-texture-filters.scene" },
    { "btn_scene_124", "scenes/124-pixelate-crt.scene" },
    { "btn_scene_125", "scenes/125-directional-light.scene" },
    { "btn_scene_126", "scenes/126-toon-and-outlines.scene" },
    { "btn_scene_127", "scenes/127-dithered-lighting.scene" },
    { "btn_scene_128", "scenes/128-mesh-from-script.scene" },
    { "btn_scene_129", "scenes/129-particle-drag-turbulence.scene" },
    { "btn_scene_130", "scenes/130-graph-position.scene" },
    { "btn_scene_131", "scenes/131-swarm-steering.scene" },
    { "btn_scene_132", "scenes/132-scene-color-in-z-order.scene" },
    { "btn_scene_133", "scenes/133-light-shafts.scene" },
    { "btn_scene_134", "scenes/134-particle-collision.scene" },
    { "btn_scene_135", "scenes/135-skeletal-blend-and-foot-ik.scene" },
    { "btn_scene_136", "scenes/136-gui-styles.scene" },
    { "btn_scene_137", "scenes/137-tilt-shift-and-turning-patrols.scene" },
    { "btn_scene_138", "scenes/138-fracture-and-caster-shadows.scene" },
    { "btn_scene_139", "scenes/139-cloth-and-rope.scene" },
    { "btn_scene_140", "scenes/140-pixel-perfect-camera.scene" },
    { "btn_scene_141", "scenes/141-cameras-into-textures.scene" },
    { "btn_scene_142", "scenes/142-sprite-masks.scene" },
    { "btn_scene_143", "scenes/143-sharp-text.scene" },
    { "btn_scene_144", "scenes/144-gpu-particles.scene" },
    { "btn_scene_145", "scenes/145-culling-masks.scene" },
    { "btn_scene_146", "scenes/146-sdf-custom-font.scene" },
    { "btn_scene_147", "scenes/147-texture-masks.scene" },
    { "btn_scene_148", "scenes/148-scene-readers-stack.scene" },
    { "btn_scene_149", "scenes/149-whole-pixel-scaling.scene" },
    { "btn_scene_150", "scenes/150-rich-text.scene" },
    { "btn_scene_151", "scenes/151-autotile.scene" },
    { "btn_scene_152", "scenes/152-isometric-tilemap.scene" },
    { "btn_scene_153", "scenes/153-carvable-sprite.scene" },
    { "btn_scene_154", "scenes/154-text-outlines.scene" },
    { "btn_scene_155", "scenes/155-spline-shape.scene" },
    { "btn_scene_156", "scenes/156-area-effects.scene" },
    { "btn_scene_157", "scenes/157-hex-tilemap.scene" },
    { "btn_scene_158", "scenes/158-sprite-shaped-shadows.scene" },
    { "btn_scene_159", "scenes/159-light-cookies.scene" },
    { "btn_scene_160", "scenes/160-animated-tiles.scene" },
    { "btn_scene_161", "scenes/161-shape-queries.scene" },
    { "btn_scene_162", "scenes/162-positional-audio.scene" },
    { "btn_scene_163", "scenes/163-navigation.scene" },
    { "btn_scene_164", "scenes/164-localization.scene" },
    { "btn_scene_165", "scenes/165-audio-effects.scene" },
    { "btn_scene_166", "scenes/166-camera-follow.scene" },
    { "btn_scene_167", "scenes/167-rumble.scene" },
    { "btn_scene_168", "scenes/168-freeform-lights.scene" },
    { "btn_scene_169", "scenes/169-jiggle-bones.scene" },
    { "btn_scene_170", "scenes/170-visibility-notifier.scene" },
    { "btn_scene_171", "scenes/171-path-follow.scene" },
    { "btn_scene_172", "scenes/172-sorting-groups.scene" },
    { "btn_scene_173", "scenes/173-lines.scene" },
    { "btn_scene_174", "scenes/174-specular.scene" },
    { "btn_scene_175", "scenes/175-canvas-groups.scene" },
    { "btn_scene_176", "scenes/176-video.scene" },
    { "btn_scene_177", "scenes/177-tile-shapes.scene" },
    { "btn_scene_178", "scenes/178-vertex-offset.scene" },
    { "btn_scene_179", "scenes/179-light-layers.scene" },
    { "btn_scene_180", "scenes/180-particle-trails.scene" },
    { "btn_scene_181", "scenes/181-emission-shapes.scene" },
    { "btn_scene_182", "scenes/182-color-grading.scene" },
    { "btn_scene_183", "scenes/183-ragdolls-and-breaking-joints.scene" },
    { "btn_scene_184", "scenes/184-split-screen.scene" },
    { "btn_scene_185", "scenes/185-keyframe-timeline.scene" },
    { "btn_scene_186", "scenes/186-font-files.scene" },
    { "btn_scene_187", "scenes/187-fixed-update.scene" },
    { "btn_scene_188", "scenes/188-ik-chains.scene" },
    { "btn_scene_189", "scenes/189-blend-spaces.scene" },
    { "btn_scene_190", "scenes/190-rich-ui-text.scene" },
    { "btn_scene_191", "scenes/191-sprite-colliders.scene" },
    { "btn_scene_192", "scenes/192-anti-aliasing.scene" },
    { "btn_scene_193", "scenes/193-conveyors.scene" },
    { "btn_scene_194", "scenes/194-middle-mouse.scene" },
    { "btn_scene_195", "scenes/195-rebinding.scene" },
    { "btn_scene_196", "scenes/196-prefab-variants.scene" },
    { "btn_scene_197", "scenes/197-camera-blend-confine.scene" },
    { "btn_scene_198", "scenes/198-bus-effects-variation.scene" },
    { "btn_scene_199", "scenes/199-sprite-swap.scene" },
    { "btn_scene_200", "scenes/200-tile-data.scene" },
    { "btn_scene_201", "scenes/201-gpu-particles-world.scene" },
    { "btn_scene_202", "scenes/202-global-illumination.scene" },
    { "btn_scene_203", "scenes/203-prefab-parts.scene" },
    { "btn_scene_204", "scenes/204-display-settings.scene" },
    { "btn_scene_205", "scenes/205-text-fields.scene" },
    { "btn_scene_206", "scenes/206-scene-streaming.scene" },
    { "btn_scene_207", "scenes/207-prompts-and-interactions.scene" },
    { "btn_scene_208", "scenes/208-music-director.scene" },
    { "btn_scene_209", "scenes/209-animation-layers.scene" },
    { "btn_scene_210", "scenes/210-particle-flipbooks.scene" },
    { "btn_scene_211", "scenes/211-noise.scene" },
    { "btn_scene_212", "scenes/212-more-joints.scene" },
    { "btn_scene_213", "scenes/213-group-materials-and-dark-lights.scene" },
    { "btn_scene_214", "scenes/214-navigation-tiles-and-obstacles.scene" },
    { "btn_scene_215", "scenes/215-spectrum-and-synth.scene" },
    { "btn_scene_216", "scenes/216-screenshots.scene" },
    { "btn_scene_217", "scenes/217-shaped-text.scene" },
    { "btn_scene_218", "scenes/218-screen-readers.scene" },
    { "btn_scene_219", "scenes/219-behaviour-trees.scene" },
    { "btn_scene_220", "scenes/220-particle-lights.scene" },
    { "btn_scene_221", "scenes/221-online-zone.scene" },
    { "btn_scene_222", "scenes/222-ui-nine-slice.scene" },
    { "btn_scene_223", "scenes/223-typed-text.scene" },
    { "btn_scene_224", "scenes/224-runtime-conformance.scene" },
    { "btn_scene_225", "scenes/225-asset-packs.scene" },
    { "btn_scene_226", "scenes/226-live-ops.scene" },
    { "btn_scene_227", "scenes/227-blur-focus.scene" },
    { "btn_scene_228", "scenes/228-paint.scene" },
    { "btn_scene_229", "scenes/229-press.scene" },
    { "btn_scene_230", "scenes/230-unlit-particles.scene" },
    { "btn_scene_231", "scenes/231-paint-canvas.scene" },
    { "btn_scene_232", "scenes/232-wet-paint.scene" },
    { "btn_scene_233", "scenes/233-fluid.scene" },
    { "btn_scene_234", "scenes/234-motion-blur.scene" },
    { "btn_scene_235", "scenes/235-lens-flare.scene" },
    { "btn_scene_236", "scenes/236-mirrors.scene" },
    { "btn_scene_237", "scenes/237-water-surface.scene" },
    { "btn_scene_238", "scenes/238-sprite-fills.scene" },
    { "btn_scene_239", "scenes/239-post-volumes.scene" },
    { "btn_scene_240", "scenes/240-pixel-sim.scene" },
    { "btn_scene_241", "scenes/241-smoke.scene" },
    { "btn_scene_242", "scenes/242-rewind.scene" },
    { "btn_scene_243", "scenes/243-wall-occlusion.scene" },
    { "btn_scene_244", "scenes/244-fog.scene" },
    { "btn_scene_245", "scenes/245-wfc.scene" },
    { "btn_scene_246", "scenes/246-vision.scene" },
    { "btn_scene_247", "scenes/247-day-cycle.scene" },
    { "btn_scene_248", "scenes/248-floating-origin.scene" },
    { "btn_scene_249", "scenes/249-time-scale.scene" },
    { "btn_scene_250", "scenes/250-cast-shadow.scene" },
    { "btn_scene_251", "scenes/251-auto-normal.scene" },
    { "btn_scene_252", "scenes/252-tile-light.scene" },
    { "btn_scene_253", "scenes/253-trajectory.scene" },
    { "btn_scene_254", "scenes/254-voronoi-shatter.scene" },
    { "btn_scene_255", "scenes/255-save-states.scene" },
    { "btn_scene_256", "scenes/256-text-path.scene" },
    { "btn_scene_257", "scenes/257-gestures.scene" },
    { "btn_scene_258", "scenes/258-depth-of-field.scene" },
    { "btn_scene_259", "scenes/259-beat-clock.scene" },
    { "btn_scene_260", "scenes/260-stateful-gpu-particles.scene" },
    { "btn_scene_261", "scenes/261-displacement-field.scene" },
    { "btn_scene_262", "scenes/262-smooth-terrain.scene" },
    { "btn_scene_263", "scenes/263-fog-of-war.scene" },
    { "btn_scene_264", "scenes/264-dialogue.scene" },
    { "btn_scene_265", "scenes/265-drag-and-drop.scene" },
    { "btn_scene_266", "scenes/266-sprite-stack.scene" },
    { "btn_scene_267", "scenes/267-mode7.scene" },
    { "btn_scene_268", "scenes/268-palette.scene" },
    { "btn_scene_269", "scenes/269-silhouette.scene" },
    { "btn_scene_270", "scenes/270-sprite-tilt.scene" },
    { "btn_scene_271", "scenes/271-afterimage.scene" },
    { "btn_scene_272", "scenes/272-paint-and-halftone.scene" },
    { "btn_scene_273", "scenes/273-scene-transition.scene" },
    { "btn_scene_274", "scenes/274-slice.scene" },
    { "btn_scene_275", "scenes/275-portal.scene" },
    { "btn_scene_276", "scenes/276-jelly.scene" },
    { "btn_scene_277", "scenes/277-offscreen-marker.scene" },
    { "btn_scene_278", "scenes/278-line-boil.scene" },
    { "btn_scene_279", "scenes/279-radial-menu.scene" },
    { "btn_scene_280", "scenes/280-lens-rain.scene" },
    { "btn_scene_281", "scenes/281-speech-bubble.scene" },
    { "btn_scene_282", "scenes/282-sprite-outline.scene" },
    { "btn_scene_283", "scenes/283-lightning.scene" },
    { "btn_scene_284", "scenes/284-flow-path.scene" },
    { "btn_scene_285", "scenes/285-snow-cover.scene" },
    { "btn_scene_286", "scenes/286-nav-links.scene" },
    { "btn_scene_287", "scenes/287-path-morph.scene" },
    { "btn_scene_288", "scenes/288-toasts.scene" },
    { "btn_scene_289", "scenes/289-input-replay.scene" },
    { "btn_scene_290", "scenes/290-mouse-delta.scene" },
    { "btn_scene_291", "scenes/291-sprite-size.scene" },
    { "btn_scene_292", "scenes/292-tile-spacing.scene" },
    { "btn_scene_293", "scenes/293-tilemap-parent.scene" },
    { "btn_scene_294", "scenes/294-tracker-songs.scene" },
    { "btn_scene_295", "scenes/295-web-platform.scene" },
    { "btn_scene_296", "scenes/296-stage.scene" },
    { "btn_scene_297", "scenes/297-mesh-normals.scene" },
    { "btn_scene_298", "scenes/298-script-maths.scene" },
    { "btn_scene_299", "scenes/299-fixed-steps.scene" },
    { "btn_scene_300", "scenes/300-lockstep.scene" },
}

function init(entity)
    for _, row in ipairs(BUTTONS) do
        local id = row[1]
        GUI.on_click(entity, GUI_PATH, id, "on_gui_click")
    end
    log("[hub] ready — " .. tostring(#BUTTONS) .. " button(s) wired")
end

-- Keyboard scrolling. The layout engine clamps `scroll_y` to the scrollable
-- range, so overshooting on either end is harmless — "99999" is how you say
-- "bottom" without knowing the content height.
local function nudge(delta)
    local current = tonumber(GUI.get(GUI_PATH, LIST_ID, "scroll_y")) or 0
    GUI.set(GUI_PATH, LIST_ID, "scroll_y", current + delta)
end

-- Up / Down (and a D-pad or stick) move focus through the buttons, and the
-- engine scrolls the focused one into view (`navigate_ui`) - scrolling here
-- as well moved the list twice. Enter / A presses the focused button.
function update(entity, dt)
    if Input.is_key_just_pressed("PageDown") then
        nudge(PAGE_STEP)
    elseif Input.is_key_just_pressed("PageUp") then
        nudge(-PAGE_STEP)
    elseif Input.is_key_just_pressed("Home") then
        GUI.set(GUI_PATH, LIST_ID, "scroll_y", 0)
    elseif Input.is_key_just_pressed("End") then
        GUI.set(GUI_PATH, LIST_ID, "scroll_y", 99999)
    end
end

function on_gui_click(self_entity, node_id)
    for _, row in ipairs(BUTTONS) do
        if row[1] == node_id then
            log("[hub] -> " .. row[2])
            Scene.load(row[2])
            return
        end
    end
    log("[hub] unknown button: " .. tostring(node_id))
end
