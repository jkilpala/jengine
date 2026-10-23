//! Translator: bridge commands → legacy `ScriptCommand` queue.
//!
//! During Phase 2 the bridge produces neutral [`super::command::BridgeCommand`] values, but
//! the engine still applies mutations through [`crate::scripting::api::ScriptCommand`]
//! via [`crate::ecs::systems::apply_script_commands`]. This translator keeps both worlds
//! consistent so adapters can be wired in shadow mode without rewriting the apply path.
//!
//! Phase 5 onward the engine will consume `BridgeCommand` directly and this translator
//! will be removed.

use crate::scripting::api::ScriptCommand;

use super::command::BridgeCommand;

/// Translate a single bridge command into the legacy command type.
pub fn to_script_command(cmd: BridgeCommand) -> ScriptCommand {
    // Default arm wraps a single ScriptCommand. The Phase 5.8 listener/timer
    // commands have no legacy ScriptCommand counterparts (the legacy backend
    // mutated EngineApi state directly), so they are dropped on the legacy
    // path via `to_script_command_opt` below.
    to_script_command_opt(cmd).expect(
        "to_script_command called on a bridge-only variant; use to_script_commands instead",
    )
}

/// Translate one bridge command, returning `None` for bridge-only variants
/// that have no legacy [`ScriptCommand`] counterpart.
pub fn to_script_command_opt(cmd: BridgeCommand) -> Option<ScriptCommand> {
    Some(match cmd {
        BridgeCommand::PackageCall { tag, args } => {
            ScriptCommand::PackageCall { tag, args }
        }
        BridgeCommand::SetPosition { entity_id, x, y } => {
            ScriptCommand::SetPosition { entity_id, x, y }
        }
        BridgeCommand::SetRotation { entity_id, radians } => {
            ScriptCommand::SetRotation { entity_id, radians }
        }
        BridgeCommand::SetScale { entity_id, x, y } => {
            ScriptCommand::SetScale { entity_id, x, y }
        }
        BridgeCommand::SetHealth { entity_id, current, max } => {
            ScriptCommand::SetHealth { entity_id, current, max }
        }
        BridgeCommand::SetHitbox { entity_id, damage, knockback, retrigger } => {
            ScriptCommand::SetHitbox { entity_id, damage, knockback, retrigger }
        }
        BridgeCommand::SetParent { child_id, parent_id } => {
            ScriptCommand::SetParent { entity_id: child_id, parent_id }
        }
        BridgeCommand::SetSkeletalPlayer { entity_id, clip, time, speed, looping, playing } => {
            ScriptCommand::SetSkeletalPlayer { entity_id, clip, time, speed, looping, playing }
        }
        BridgeCommand::Log { message } => ScriptCommand::Log { message },
        BridgeCommand::SpawnEntity { name, prefab: _, spawn_id } => {
            // Phase 5.1: prefab lookup is intentionally dropped during shadow-mode
            // translation — the bridge runner does not own the prefab table. The
            // legacy path is reached via `EngineApi::spawn_prefab` directly; the
            // bridge command form is here for parity test consumers.
            ScriptCommand::SpawnEntity {
                name,
                components: Box::new(None),
                spawn_id: Some(spawn_id),
                children: Vec::new(),
            }
        }
        BridgeCommand::SpawnRig { rig_id, spawn_id } => {
            ScriptCommand::SpawnRig { rig_id, spawn_id }
        }
        BridgeCommand::SetIkTarget { entity_id, x, y } => {
            ScriptCommand::SetIkTarget { entity_id, x, y }
        }
        BridgeCommand::CrossfadeAnimation { entity_id, clip, seconds } => {
            ScriptCommand::CrossfadeAnimation { entity_id, clip, seconds }
        }
        BridgeCommand::SetAnimationBlend { entity_id, clip, weight } => {
            ScriptCommand::SetAnimationBlend { entity_id, clip, weight }
        }
        BridgeCommand::SetAnimationBlendPosition { entity_id, x, y } => {
            ScriptCommand::SetAnimationBlendPosition { entity_id, x, y }
        }
        BridgeCommand::AnimationLayer { entity_id, op, layer, clip, value, seconds, looping } => {
            ScriptCommand::AnimationLayer { entity_id, op, layer, clip, value, seconds, looping }
        }
        BridgeCommand::SetConveyorSpeed { entity_id, speed } => ScriptCommand::SetConveyorSpeed { entity_id, speed },
        BridgeCommand::SetIkWeight { entity_id, weight } => ScriptCommand::SetIkWeight { entity_id, weight },
        BridgeCommand::DestroyEntity { entity_id } => {
            ScriptCommand::DestroyEntity { entity_id }
        }
        BridgeCommand::AddTag { entity_id, tag } => ScriptCommand::AddTag { entity_id, tag },
        BridgeCommand::RemoveTag { entity_id, tag } => ScriptCommand::RemoveTag { entity_id, tag },
        BridgeCommand::SetSpriteColor { entity_id, r, g, b, a } => {
            ScriptCommand::SetSpriteColor { entity_id, r, g, b, a }
        }
        BridgeCommand::SetCameraZoom { zoom } => ScriptCommand::SetCameraZoom { zoom },
        BridgeCommand::SetCameraPixelSize { size } => ScriptCommand::SetCameraPixelSize { size },
        BridgeCommand::SetZOrder { entity_id, z } => ScriptCommand::SetZOrder { entity_id, z },
        BridgeCommand::SetTextColor { entity_id, r, g, b, a } => ScriptCommand::SetTextColor { entity_id, r, g, b, a },
        BridgeCommand::SetTrailEmitting { entity_id, on } => ScriptCommand::SetTrailEmitting { entity_id, on },
        BridgeCommand::SetMesh { entity_id, vertices, indices } => ScriptCommand::SetMesh { entity_id, vertices, indices },
        BridgeCommand::ClearTrail { entity_id } => ScriptCommand::ClearTrail { entity_id },
        BridgeCommand::CarveSprite { entity_id, x, y, radius } => ScriptCommand::CarveSprite { entity_id, x, y, radius },
        BridgeCommand::PaintSprite { entity_id, x, y, size, color, shape, rotation, depth, blend } => {
            ScriptCommand::PaintSprite { entity_id, x, y, size, color, shape, rotation, depth, blend }
        }
        BridgeCommand::MoveCharacter { entity_id, vx, vy, drop_through } => {
            ScriptCommand::MoveCharacter { entity_id, vx, vy, drop_through }
        }
        BridgeCommand::SetSpriteSize { entity_id, w, h } => ScriptCommand::SetSpriteSize { entity_id, w, h },
        BridgeCommand::SetSpriteAnchor { entity_id, x, y } => {
            ScriptCommand::SetSpriteAnchor { entity_id, x, y }
        }
        BridgeCommand::SetSpriteLabel { entity_id, label, category } => {
            ScriptCommand::SetSpriteLabel { entity_id, label, category }
        }
        BridgeCommand::SetSpriteRegion { entity_id, atlas_id, region_name } => {
            ScriptCommand::SetSpriteRegion { entity_id, atlas_id, region_name }
        }
        BridgeCommand::SetSpriteCorners { entity_id, corners } => {
            ScriptCommand::SetSpriteCorners { entity_id, corners }
        }
        BridgeCommand::SetMaterial { entity_id, material_id } => {
            ScriptCommand::SetMaterial { entity_id, material_id }
        }
        BridgeCommand::SetNormalMap { entity_id, normal_map_id } => {
            ScriptCommand::SetNormalMap { entity_id, normal_map_id }
        }
        BridgeCommand::SetMaterialUniformFloat { entity_id, name, value } => {
            ScriptCommand::SetMaterialUniform {
                entity_id,
                name,
                value: crate::renderer::material::UniformValue::Float(value),
            }
        }
        BridgeCommand::SetMaterialUniformVec2 { entity_id, name, value } => {
            ScriptCommand::SetMaterialUniform {
                entity_id,
                name,
                value: crate::renderer::material::UniformValue::Vec2(value),
            }
        }
        BridgeCommand::SetMaterialUniformInt { entity_id, name, value } => {
            ScriptCommand::SetMaterialUniform {
                entity_id,
                name,
                value: crate::renderer::material::UniformValue::Int(value),
            }
        }
        BridgeCommand::SetMaterialUniformVec4 { entity_id, name, value } => {
            ScriptCommand::SetMaterialUniform {
                entity_id,
                name,
                value: crate::renderer::material::UniformValue::Vec4(value),
            }
        }
        BridgeCommand::ApplyForce { entity_id, fx, fy } => {
            ScriptCommand::ApplyForce { entity_id, fx, fy }
        }
        BridgeCommand::ApplyImpulse { entity_id, ix, iy } => {
            ScriptCommand::ApplyImpulse { entity_id, ix, iy }
        }
        BridgeCommand::SetVelocity { entity_id, vx, vy } => {
            ScriptCommand::SetVelocity { entity_id, vx, vy }
        }
        BridgeCommand::SetAngularVelocity { entity_id, omega } => {
            ScriptCommand::SetAngularVelocity { entity_id, omega }
        }
        // The bridge `sound_id` is the id handed to the script: carried
        // through, so the host can say which backend sound it became and
        // `stop_sound` / `set_sound_volume` / `set_sound_pitch` reach it.
        // It used to be dropped here, so none of them ever did.
        BridgeCommand::PlaySound { clip_id, volume, pitch, pan, sound_id, looping } => {
            ScriptCommand::PlaySound { clip_id, volume, pitch, pan, sound_id, looping }
        }
        BridgeCommand::PlayMusic { clip_id, volume, looping } => {
            ScriptCommand::PlayMusic { clip_id, volume, looping }
        }
        BridgeCommand::StopSound { handle_id } => ScriptCommand::StopSound { handle_id },
        BridgeCommand::StopSoundByClip { clip_id } => ScriptCommand::StopSoundByClip { clip_id },
        BridgeCommand::StopMusic => ScriptCommand::StopMusic,
        BridgeCommand::PauseMusic => ScriptCommand::PauseMusic,
        BridgeCommand::ResumeMusic => ScriptCommand::ResumeMusic,
        BridgeCommand::SetSoundVolume { handle_id, volume } => {
            ScriptCommand::SetSoundVolume { handle_id, volume }
        }
        BridgeCommand::SetMusicVolume { volume } => ScriptCommand::SetMusicVolume { volume },
        BridgeCommand::SetMasterVolume { volume } => ScriptCommand::SetMasterVolume { volume },
        BridgeCommand::StopAllSounds => ScriptCommand::StopAllSounds,
        BridgeCommand::PauseAllSounds => ScriptCommand::PauseAllSounds,
        BridgeCommand::ResumeAllSounds => ScriptCommand::ResumeAllSounds,
        // Phase 5.6: animation
        BridgeCommand::PlayAnimation { entity_id, clip_name } => {
            ScriptCommand::PlayAnimation { entity_id, clip_name }
        }
        BridgeCommand::StopAnimation { entity_id } => ScriptCommand::StopAnimation { entity_id },
        BridgeCommand::SetAnimationSpeed { entity_id, speed } => {
            ScriptCommand::SetAnimationSpeed { entity_id, speed }
        }
        BridgeCommand::SetAnimBool { entity_id, param, value } => {
            ScriptCommand::SetAnimBool { entity_id, param, value }
        }
        BridgeCommand::SetAnimFloat { entity_id, param, value } => {
            ScriptCommand::SetAnimFloat { entity_id, param, value }
        }
        BridgeCommand::SetAnimTrigger { entity_id, param } => {
            ScriptCommand::SetAnimTrigger { entity_id, param }
        }
        BridgeCommand::StartStateMachine { entity_id, state_name } => {
            ScriptCommand::StartStateMachine { entity_id, state_name }
        }
        // The legacy `apply_script_commands` no-ops StartTween/CancelTween/CancelAllTweens
        // because tweens are applied directly via `EngineApi::tween_*` against
        // the manager. Translation here keeps the variant for parity-test
        // observability; the bridge-side `tween_id` is dropped.
        BridgeCommand::StartTween { entity_id, target, end_value, duration, easing, tween_id: _ } => {
            ScriptCommand::StartTween {
                entity_id,
                target: parse_tween_target(&target),
                end_value,
                duration,
                easing: crate::animation::easing::EasingFunction::from_name(&easing),
            }
        }
        BridgeCommand::CancelTween { tween_id } => ScriptCommand::CancelTween { tween_id },
        BridgeCommand::CancelAllTweens { entity_id } => {
            ScriptCommand::CancelAllTweens { entity_id }
        }
        // Phase 5.7: scene
        BridgeCommand::LoadScene { path } => ScriptCommand::LoadScene { path },
        BridgeCommand::QuitGame => ScriptCommand::QuitGame,
        BridgeCommand::SetTimeScale { scale } => ScriptCommand::SetTimeScale { scale },
        // Physics writes — the apply arms already exist in `ecs/systems.rs`.
        BridgeCommand::SetGravity { gx, gy } => ScriptCommand::SetGravity { gx, gy },
        BridgeCommand::SetBodyType {
            entity_id,
            body_type,
        } => ScriptCommand::SetBodyType {
            entity_id,
            body_type,
        },
        BridgeCommand::AddJoint {
            entity_name,
            target_name,
            joint_json,
        } => ScriptCommand::AddJoint {
            entity_name,
            target_name,
            joint_json,
        },
        BridgeCommand::RemoveJoint { entity_name } => {
            ScriptCommand::RemoveJoint { entity_name }
        }
        BridgeCommand::SetMotorSpeed { entity_name, speed } => {
            ScriptCommand::SetMotorSpeed { entity_name, speed }
        }
        BridgeCommand::PushScene { path } => ScriptCommand::PushScene { path },
        BridgeCommand::PopScene => ScriptCommand::PopScene,
        // Slice 12.A — fade transitions.
        BridgeCommand::FadeIn { duration_s, color } => {
            ScriptCommand::FadeIn { duration_s, color }
        }
        BridgeCommand::FadeOut { duration_s, color } => {
            ScriptCommand::FadeOut { duration_s, color }
        }
        // P1.3 — screen flash.
        BridgeCommand::ScreenFlash { duration_s, color } => {
            ScriptCommand::ScreenFlash { duration_s, color }
        }
        // P7.8: dynamic script loading is bridge-only — handled directly in
        // route_bridge_commands by adding a ScriptComponent to the entity.
        BridgeCommand::LoadScript { .. } => return None,
        // Phase 5.8: events & timers.
        // SendEvent maps directly. Listener register/remove and timer
        // schedule/cancel have no legacy ScriptCommand counterparts — the
        // legacy backend mutated `EngineApi` state directly. They are dropped
        // here so existing parity tests that round-trip through translation
        // keep working; the bridge-side `BridgeCommand` queue still records
        // them for the new-path consumer.
        BridgeCommand::SendEvent { name, sender, data } => ScriptCommand::SendEvent {
            name,
            sender,
            // Decoded as the browser hosts decode it: JSON text is the
            // value it spells (a table arrives as a table), anything else
            // the string it is.
            data: data.map(|d| crate::scripting::host_commands::event_handler_data(Some(&d))),
        },
        BridgeCommand::RegisterEventListener { .. }
        | BridgeCommand::RemoveEventListener { .. }
        | BridgeCommand::SetTimer { .. }
        | BridgeCommand::CancelTimer { .. } => return None,
        // Phase 5.9: UI / GUI.
        // Anchor strings are parsed via `UIAnchor::from_name` (defaults to
        // TopLeft for unknown names — same behaviour as the legacy backends).
        BridgeCommand::CreateUIText {
            text,
            anchor,
            offset_x,
            offset_y,
            color,
            font_size,
            alignment,
            entity_id,
        } => {
            ScriptCommand::CreateUIText {
                name: format!("UIText_{entity_id}"),
                text,
                anchor: crate::ui::UIAnchor::from_name(&anchor),
                offset_x,
                offset_y,
                color,
                font_size,
                alignment: parse_text_align(&alignment),
            }
        }
        BridgeCommand::CreateUIPanel {
            anchor,
            offset_x,
            offset_y,
            width,
            height,
            bg_color,
            entity_id,
        } => ScriptCommand::CreateUIPanel {
            name: format!("UIPanel_{entity_id}"),
            anchor: crate::ui::UIAnchor::from_name(&anchor),
            offset_x,
            offset_y,
            width,
            height,
            bg_color,
        },
        BridgeCommand::SetUIText { entity_id, text } => {
            ScriptCommand::SetUIText { entity_id, text }
        }
        BridgeCommand::SetUiProp { entity_id, property, value } => {
            ScriptCommand::SetUiProp { entity_id, property, value }
        }
        BridgeCommand::SetText { entity_id, text } => {
            ScriptCommand::SetText { entity_id, text }
        }
        BridgeCommand::SetUIVisible { entity_id, visible } => {
            ScriptCommand::SetUIVisible { entity_id, visible }
        }
        BridgeCommand::SetUIPosition { entity_id, anchor, offset_x, offset_y } => {
            ScriptCommand::SetUIPosition {
                entity_id,
                anchor: crate::ui::UIAnchor::from_name(&anchor),
                offset_x,
                offset_y,
            }
        }
        BridgeCommand::GuiShow { gui_id } => ScriptCommand::GuiShow { gui_id },
        BridgeCommand::GuiHide { gui_id } => ScriptCommand::GuiHide { gui_id },
        BridgeCommand::GuiSetNodeProp { gui_id, node_id, property, value } => {
            // Scripts pass `value` as a stringified scalar (Lua/JS bridges flatten
            // to String at the FFI boundary). Try to parse it as JSON first so
            // numbers/bools/null reach the handler with the correct type; fall
            // back to a plain string for text properties.
            let json_value = crate::ui::gui_document::gui_prop_value(&property, value);
            ScriptCommand::GuiSetNodeProp {
                gui_id,
                node_id,
                property,
                value: json_value,
            }
        }
        BridgeCommand::GuiFocus { gui_id, node_id } => {
            ScriptCommand::GuiFocus { gui_id, node_id }
        }
        // `gui_on_click` mutates `EngineApi::gui_click_handlers` directly in
        // the legacy backend — there is no matching ScriptCommand. Drop on
        // translation; the bridge-side queue still records the registration.
        BridgeCommand::GuiOnClick { .. } => return None,
        BridgeCommand::EmitBurst { entity_id, count } => {
            ScriptCommand::EmitBurst { entity_id, count }
        }
        BridgeCommand::SetEmitterActive { entity_id, active } => {
            ScriptCommand::SetEmitterActive { entity_id, active }
        }
        BridgeCommand::SetEmitterRate { entity_id, rate } => {
            ScriptCommand::SetEmitterRate { entity_id, rate }
        }
        BridgeCommand::SetVideoField { entity_id, field, value } => {
            ScriptCommand::SetVideoField { entity_id, field, value }
        }
        BridgeCommand::SetRagdollField { entity_id, field, value } => {
            ScriptCommand::SetRagdollField { entity_id, field, value }
        }
        BridgeCommand::SetBehaviorTreeField { entity_id, field, value } => {
            ScriptCommand::SetBehaviorTreeField { entity_id, field, value }
        }
        BridgeCommand::SetTimelineField { entity_id, field, value } => {
            ScriptCommand::SetTimelineField { entity_id, field, value }
        }
        BridgeCommand::SetCanvasGroupField { entity_id, field, value } => {
            ScriptCommand::SetCanvasGroupField { entity_id, field, value }
        }
        BridgeCommand::SetFluidField { entity_id, field, value } => {
            ScriptCommand::SetFluidField { entity_id, field, value }
        }
        BridgeCommand::SetMirrorField { entity_id, field, value } => {
            ScriptCommand::SetMirrorField { entity_id, field, value }
        }
        BridgeCommand::SetSpriteFill { entity_id, amount } => ScriptCommand::SetSpriteFill { entity_id, amount },
        BridgeCommand::WfcGenerate { entity_id, x, y, w, h } => ScriptCommand::WfcGenerate { entity_id, x, y, w, h },
        BridgeCommand::TerrainEdit { entity_id, x, y, radius, amount } => {
            ScriptCommand::TerrainEdit { entity_id, x, y, radius, amount }
        }
        BridgeCommand::DialogueOp { entity_id, op, arg } => ScriptCommand::DialogueOp { entity_id, op, arg },
        BridgeCommand::UiDrop { entity_id, slot } => ScriptCommand::UiDrop { entity_id, slot },
        BridgeCommand::Mode7Place { entity_id, x, y, angle } => ScriptCommand::Mode7Place { entity_id, x, y, angle },
        BridgeCommand::Mode7Set { entity_id, field, value } => ScriptCommand::Mode7Set { entity_id, field, value },
        BridgeCommand::SpriteTiltSet { entity_id, field, value } => ScriptCommand::SpriteTiltSet { entity_id, field, value },
        BridgeCommand::AfterimageSet { entity_id, field, value } => ScriptCommand::AfterimageSet { entity_id, field, value },
        BridgeCommand::PhysicsSlice { x0, y0, x1, y1 } => ScriptCommand::PhysicsSlice { x0, y0, x1, y1 },
        BridgeCommand::PortalSet { entity_id, field, value } => ScriptCommand::PortalSet { entity_id, field, value },
        BridgeCommand::JellyPoke { entity_id, x, y, strength, radius } => ScriptCommand::JellyPoke { entity_id, x, y, strength, radius },
        BridgeCommand::JellySet { entity_id, field, value } => ScriptCommand::JellySet { entity_id, field, value },
        BridgeCommand::MarkerSet { entity_id, field, value } => ScriptCommand::MarkerSet { entity_id, field, value },
        BridgeCommand::RadialSet { entity_id, field, value } => ScriptCommand::RadialSet { entity_id, field, value },
        BridgeCommand::BubbleSay { entity_id, text, seconds } => ScriptCommand::BubbleSay { entity_id, text, seconds },
        BridgeCommand::BubbleSet { entity_id, field, value } => ScriptCommand::BubbleSet { entity_id, field, value },
        BridgeCommand::OutlineSet { entity_id, field, value } => ScriptCommand::OutlineSet { entity_id, field, value },
        BridgeCommand::LightningStrike { entity_id, x, y } => ScriptCommand::LightningStrike { entity_id, x, y },
        BridgeCommand::LightningSet { entity_id, field, value } => ScriptCommand::LightningSet { entity_id, field, value },
        BridgeCommand::FlowSet { entity_id, field, value } => ScriptCommand::FlowSet { entity_id, field, value },
        BridgeCommand::NavLinkSet { entity_id, field, value } => ScriptCommand::NavLinkSet { entity_id, field, value },
        BridgeCommand::MorphTo { entity_id, target, seconds } => ScriptCommand::MorphTo { entity_id, target, seconds },
        BridgeCommand::MorphSet { entity_id, field, value } => ScriptCommand::MorphSet { entity_id, field, value },
        BridgeCommand::ToastShow { entity_id, title, text, icon, seconds } => ScriptCommand::ToastShow { entity_id, title, text, icon, seconds },
        BridgeCommand::ToastSet { entity_id, field, value } => ScriptCommand::ToastSet { entity_id, field, value },
        BridgeCommand::SnowSet { entity_id, field, value } => ScriptCommand::SnowSet { entity_id, field, value },
        BridgeCommand::FogOfWarReset { entity_id } => ScriptCommand::FogOfWarReset { entity_id },
        BridgeCommand::FogOfWarSet { entity_id, field, value } => {
            ScriptCommand::FogOfWarSet { entity_id, field, value }
        }
        BridgeCommand::SetBeatClockField { entity_id, field, value } => {
            ScriptCommand::SetBeatClockField { entity_id, field, value }
        }
        BridgeCommand::SetTextPathField { entity_id, field, value } => {
            ScriptCommand::SetTextPathField { entity_id, field, value }
        }
        BridgeCommand::SetTrajectoryField { entity_id, field, value } => {
            ScriptCommand::SetTrajectoryField { entity_id, field, value }
        }
        BridgeCommand::SetTileLightField { entity_id, field, value } => {
            ScriptCommand::SetTileLightField { entity_id, field, value }
        }
        BridgeCommand::SetAutoNormalField { entity_id, field, value } => {
            ScriptCommand::SetAutoNormalField { entity_id, field, value }
        }
        BridgeCommand::SetCastShadowField { entity_id, field, value } => {
            ScriptCommand::SetCastShadowField { entity_id, field, value }
        }
        BridgeCommand::SetTimeScaleField { entity_id, field, value } => {
            ScriptCommand::SetTimeScaleField { entity_id, field, value }
        }
        BridgeCommand::SetOriginField { entity_id, field, value } => {
            ScriptCommand::SetOriginField { entity_id, field, value }
        }
        BridgeCommand::SetDayField { entity_id, field, value } => ScriptCommand::SetDayField { entity_id, field, value },
        BridgeCommand::SetVisionField { entity_id, field, value } => {
            ScriptCommand::SetVisionField { entity_id, field, value }
        }
        BridgeCommand::SetFogField { entity_id, field, value } => {
            ScriptCommand::SetFogField { entity_id, field, value }
        }
        BridgeCommand::SetRewindField { entity_id, field, value } => {
            ScriptCommand::SetRewindField { entity_id, field, value }
        }
        BridgeCommand::SmokePuff { entity_id, x, y, radius, amount, vx, vy } => {
            ScriptCommand::SmokePuff { entity_id, x, y, radius, amount, vx, vy }
        }
        BridgeCommand::SetSmokeField { entity_id, field, value } => {
            ScriptCommand::SetSmokeField { entity_id, field, value }
        }
        BridgeCommand::PixelPaint { entity_id, x, y, radius, material } => {
            ScriptCommand::PixelPaint { entity_id, x, y, radius, material }
        }
        BridgeCommand::SetWaterField { entity_id, field, value } => {
            ScriptCommand::SetWaterField { entity_id, field, value }
        }
        BridgeCommand::WaterSplash { entity_id, x, strength } => ScriptCommand::WaterSplash { entity_id, x, strength },
        BridgeCommand::SetTrailField { entity_id, field, value } => {
            ScriptCommand::SetTrailField { entity_id, field, value }
        }
        BridgeCommand::SetLineField { entity_id, field, value } => {
            ScriptCommand::SetLineField { entity_id, field, value }
        }
        BridgeCommand::SetPathFollowField { entity_id, field, value } => {
            ScriptCommand::SetPathFollowField { entity_id, field, value }
        }
        BridgeCommand::SetClothField { entity_id, field, value } => {
            ScriptCommand::SetClothField { entity_id, field, value }
        }
        BridgeCommand::SetAreaField { entity_id, field, value } => ScriptCommand::SetAreaField { entity_id, field, value },
        BridgeCommand::SetCameraField { entity_id, field, value } => ScriptCommand::SetCameraField { entity_id, field, value },
        BridgeCommand::SetCameraFollow { entity_id, field, value } => ScriptCommand::SetCameraFollow { entity_id, field, value },
        BridgeCommand::SetCameraScreenRect { entity_id, rect } => ScriptCommand::SetCameraScreenRect { entity_id, rect },
        BridgeCommand::SetNavTarget { entity_id, target } => ScriptCommand::SetNavTarget { entity_id, target },
        BridgeCommand::SetNavField { entity_id, field, value } => ScriptCommand::SetNavField { entity_id, field, value },
        BridgeCommand::SetEmitterField { entity_id, field, value } => {
            ScriptCommand::SetEmitterField { entity_id, field, value }
        }
        BridgeCommand::SetTile { entity_id, layer_name, x, y, tile_id } => {
            ScriptCommand::SetTile { entity_id, layer_name, x, y, tile_id }
        }
        BridgeCommand::SetLightColor { entity_id, r, g, b } => {
            ScriptCommand::SetLightColor { entity_id, r, g, b }
        }
        BridgeCommand::SetLightIntensity { entity_id, intensity } => {
            ScriptCommand::SetLightIntensity { entity_id, intensity }
        }
        BridgeCommand::SetLightHeight { entity_id, height } => ScriptCommand::SetLightHeight { entity_id, height },
        BridgeCommand::SetLightMask { entity_id, mask } => ScriptCommand::SetLightMask { entity_id, mask },
        BridgeCommand::SetColorLut { texture_id, strength } => ScriptCommand::SetColorLut { texture_id, strength },
        BridgeCommand::SetLightRadius { entity_id, radius } => {
            ScriptCommand::SetLightRadius { entity_id, radius }
        }
        BridgeCommand::SetAmbientLight { r, g, b, intensity } => {
            ScriptCommand::SetAmbientLight { r, g, b, intensity }
        }
        BridgeCommand::SetToonLighting { bands, softness, dither } => {
            ScriptCommand::SetToonLighting { bands, softness, dither }
        }
        BridgeCommand::SetAmbientOcclusion { radius, strength } => {
            ScriptCommand::SetAmbientOcclusion { radius, strength }
        }
        BridgeCommand::SetGlobalIllumination { strength, scale } => {
            ScriptCommand::SetGlobalIllumination { strength, scale }
        }
        BridgeCommand::SetPostProcessEnabled { effect_name, enabled } => {
            ScriptCommand::SetPostProcessEnabled { effect_name, enabled }
        }
        BridgeCommand::SetPostProcessFloat { effect_name, param_name, value } => {
            ScriptCommand::SetPostProcessParam {
                effect_name,
                param_name,
                value: crate::renderer::material::UniformValue::Float(value),
            }
        }
        BridgeCommand::SetPostProcessVec4 { effect_name, param_name, value } => {
            ScriptCommand::SetPostProcessParam {
                effect_name,
                param_name,
                value: crate::renderer::material::UniformValue::Vec4(value),
            }
        }
        // Debug draws + overlay are bridge-only — no matching ScriptCommand.
        // The host runner reads them straight off the BridgeCommand queue.
        BridgeCommand::DrawDebugLine { .. }
        | BridgeCommand::DrawDebugRect { .. }
        | BridgeCommand::DrawDebugCircle { .. }
        | BridgeCommand::SetDebugOverlay { .. }
        | BridgeCommand::CameraShake { .. } => return None,
        // Phase 5.13: 1:1 mapping onto the legacy custom-component setter.
        BridgeCommand::SetCustomField {
            entity_id,
            component_name,
            field_name,
            value,
        } => ScriptCommand::SetCustomField {
            entity_id,
            component_name: component_name.to_string(),
            field_name: field_name.to_string(),
            value,
        },
        // Expanded to one SetCustomField per field by `to_script_commands`.
        BridgeCommand::SetCustomFields { .. } => return None,
        BridgeCommand::SetComponentField { .. } => return None, // Bridge-only
        // Phase C: targeted comms — bridge-only, runner dispatches directly.
        BridgeCommand::SendEventTo { .. } => return None,
        BridgeCommand::CallScriptMethod { .. } => return None,
        // 2.3 save system — bridge-only; runner applies to `SaveSystem`.
        BridgeCommand::SaveSet { .. }
        | BridgeCommand::SaveCommit { .. }
        | BridgeCommand::SaveClear { .. } => return None,
        // 2.4 audio mixer — bridge-only; runner applies to `AudioMixer`.
        BridgeCommand::AudioCreateBus { .. }
        | BridgeCommand::AudioRemoveBus { .. }
        | BridgeCommand::AudioSetBusVolume { .. }
        | BridgeCommand::AudioSetBusEffect { .. }
        | BridgeCommand::AudioSetBusMuted { .. }
        | BridgeCommand::AudioSetBusSolo { .. } => return None,
        // 2.5 input remapping — bridge-only; runner mutates `InputState`.
        BridgeCommand::InputSetBindings { .. }
        | BridgeCommand::InputAddBinding { .. }
        | BridgeCommand::InputRemoveBinding { .. }
        | BridgeCommand::InputClearBindings { .. }
        | BridgeCommand::InputResetBindings
        | BridgeCommand::InputSetDeadZone { .. } => return None,
    })
}

/// Parse the bridge's string `target` into the legacy [`crate::animation::tween::TweenTarget`].
/// Defaults to `Position` for unrecognized names so translation is total
/// (parity tests assert on the `BridgeCommand` shape, not the post-translation
/// enum, so an unknown name there will still surface as a mismatch on the
/// bridge side).
pub(crate) fn parse_tween_target(name: &str) -> crate::animation::tween::TweenTarget {
    use crate::animation::tween::TweenTarget;
    match name {
        "rotation" => TweenTarget::Rotation,
        "scale" => TweenTarget::Scale,
        "sprite_color" => TweenTarget::SpriteColor,
        "sprite_alpha" => TweenTarget::SpriteAlpha,
        _ => TweenTarget::Position,
    }
}

/// Parse a script-side alignment string ("left" / "center" / "right",
/// case-insensitive) into the engine's [`crate::ui::TextAlign`] enum.
/// Unknown names fall back to `Left` (matches `UIText::default`).
fn parse_text_align(name: &str) -> crate::ui::TextAlign {
    use crate::ui::TextAlign;
    match name.to_ascii_lowercase().as_str() {
        "center" | "middle" => TextAlign::Center,
        "right" => TextAlign::Right,
        _ => TextAlign::Left,
    }
}

/// Translate a batch of bridge commands.
///
/// `SetCustomFields` (from `set_fields`) is one bridge crossing carrying many
/// fields, and the apply side only has the single-field setter, so it expands
/// here into one `SetCustomField` per field. It used to go through
/// [`to_script_command_opt`] like everything else, which maps it to nothing -
/// and no runner handled it either, so native `set_fields` wrote nothing.
pub fn to_script_commands<I: IntoIterator<Item = BridgeCommand>>(iter: I) -> Vec<ScriptCommand> {
    let mut out = Vec::new();
    for cmd in iter {
        match cmd {
            BridgeCommand::SetCustomFields {
                entity_id,
                component_name,
                fields,
            } => {
                for (field_name, value) in fields {
                    out.push(ScriptCommand::SetCustomField {
                        entity_id,
                        component_name: component_name.to_string(),
                        field_name: field_name.to_string(),
                        value,
                    });
                }
            }
            other => out.extend(to_script_command_opt(other)),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_each_variant() {
        let out = to_script_commands(vec![
            BridgeCommand::SetPosition { entity_id: 1, x: 2.0, y: 3.0 },
            BridgeCommand::SetRotation { entity_id: 1, radians: 0.5 },
            BridgeCommand::SetScale { entity_id: 1, x: 2.0, y: 2.0 },
            BridgeCommand::Log { message: "hi".into() },
        ]);
        assert_eq!(out.len(), 4);
        assert!(matches!(out[0], ScriptCommand::SetPosition { entity_id: 1, .. }));
        assert!(matches!(out[1], ScriptCommand::SetRotation { entity_id: 1, .. }));
        assert!(matches!(out[2], ScriptCommand::SetScale { entity_id: 1, .. }));
        assert!(matches!(out[3], ScriptCommand::Log { .. }));
    }
}
