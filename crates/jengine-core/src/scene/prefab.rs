//! 2.2 — Prefab variants / overrides.
//!
//! Expands `PrefabInstance` markers in a [`SceneData`] by merging a
//! prefab template's component bundle onto the entity, then applying
//! dotted-path per-instance overrides on top. Run between scene
//! parsing and `load_scene` so a single Player prefab can be re-skinned
//! per scene without forking the template.
//!
//! ## Merge order
//!
//! 1. Start with the prefab template's `ComponentBundle`.
//! 2. Overlay any components the scene already authored on the
//!    instance — authored components win over the template.
//! 3. Apply `prefab_instance.overrides` dotted-path patches on top.
//!    Override paths can target a whole component (`"sprite"`) or a
//!    nested field (`"sprite.color"`, `"transform.position"`).
//!
//! The `PrefabInstance` component itself is retained on the resulting
//! entity as a marker so scripts/tooling can ask "what prefab did this
//! come from?".
//!
//! ## Naming
//!
//! If the scene's instance entity has an empty `name`, the prefab
//! template's name is used.

use std::collections::HashMap;

use serde_json::Value;

use super::format::{ComponentBundle, EntityData, SceneData};

/// Result of expanding prefab instances in a scene.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ExpandReport {
    /// Number of `PrefabInstance` markers successfully resolved.
    pub expanded: usize,
    /// `prefab_id`s referenced that weren't in the prefab map. Each
    /// missing entry is left untouched (the marker stays on the
    /// entity); the caller can log / warn.
    pub missing: Vec<String>,
    /// Prefab parts hung out as entities of their own (`EntityData::children`).
    pub parts: usize,
}

/// Expand `prefab_instance` entries in a scene JSON string, returning the
/// rewritten JSON.
///
/// The JSON plumbing around [`expand_prefab_instances`], lifted here because
/// it was copied verbatim into `jengine-runtime` and `jengine-web` while the
/// editor viewport had no copy at all — so an entity whose content came from a
/// `PrefabInstance` marker was simply empty in the editor, the surface most of
/// the authoring happens on.
///
/// Anything it cannot confidently rewrite passes through unchanged — a parse
/// error, or a v2 scene, whose `ComponentRegistry` dispatch expansion is a
/// separate wedge — so the downstream loader still surfaces the real
/// diagnostic instead of this function masking it.
///
/// `context` prefixes the log lines with the runtime doing the expanding,
/// since the three call sites are otherwise indistinguishable in a log.
pub fn expand_prefab_instances_in_json(
    json: &str,
    prefabs: &std::collections::HashMap<String, crate::scene::format::EntityData>,
    context: &str,
) -> String {
    let raw: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return json.to_string(),
    };
    if raw.get("version").and_then(|v| v.as_u64()).unwrap_or(1) >= 2 {
        let mut raw = raw;
        let report = expand_v2_instances(&mut raw, prefabs);
        if report.expanded == 0 && report.missing.is_empty() && report.parts == 0 {
            return json.to_string();
        }
        if report.expanded > 0 {
            log::info!("[{context}] expanded {} prefab_instance entry/entries", report.expanded);
        }
        if !report.missing.is_empty() {
            log::warn!(
                "[{context}] {} prefab_instance entry/entries reference missing prefab(s): {:?}",
                report.missing.len(),
                report.missing
            );
        }
        return serde_json::to_string(&raw).unwrap_or_else(|_| json.to_string());
    }
    // `SceneData` does not model every top-level field a scene can carry
    // (`audio_refs` is the one that bites), and a typed round-trip drops what
    // it cannot see — silently, so the loss only surfaces later as a sound that
    // never plays. Keep the original object to merge back.
    //
    // The web copy of this function did that and the native one did not, so
    // until this was consolidated a native scene using prefab instances lost
    // its audio refs.
    let original = raw.clone();
    let mut scene_data: crate::scene::SceneData = match serde_json::from_value(raw) {
        Ok(s) => s,
        Err(_) => return json.to_string(),
    };
    // Tiles that are things become prefab instances first, so they are
    // expanded with the rest (`tilemap::scene_tiles`).
    let spawned = crate::tilemap::scene_tiles::expand_scene_tiles(&mut scene_data);
    if spawned > 0 {
        log::info!("[{context}] {spawned} tile(s) became prefab instances");
    }
    let report = expand_prefab_instances(&mut scene_data, prefabs);
    if report.expanded > 0 {
        log::info!("[{context}] expanded {} prefab_instance entry/entries", report.expanded);
    }
    if !report.missing.is_empty() {
        log::warn!(
            "[{context}] {} prefab_instance entry/entries reference missing prefab(s): {:?}",
            report.missing.len(),
            report.missing
        );
    }
    let mut out = match serde_json::to_value(&scene_data) {
        Ok(v) => v,
        Err(_) => return json.to_string(),
    };
    // Restore any top-level key the round-trip lost. `or_insert` means the
    // expanded `entities` (and everything else the typed form owns) wins.
    if let (Some(out_obj), Some(orig_obj)) = (out.as_object_mut(), original.as_object()) {
        for (key, value) in orig_obj {
            out_obj.entry(key.clone()).or_insert_with(|| value.clone());
        }
    }
    serde_json::to_string(&out).unwrap_or_else(|_| json.to_string())
}

/// Walk `scene.entities`; for each entity with a `PrefabInstance`
/// component, merge the template + apply overrides. See module docs.
pub fn expand_prefab_instances(
    scene: &mut SceneData,
    prefabs: &HashMap<String, EntityData>,
) -> ExpandReport {
    let mut report = ExpandReport::default();
    // In rounds: expand the instances, then hang every child out as an
    // entity of its own (`flatten_children`), whose own instances the next
    // round expands - a prefab built of prefabs, a few levels deep.
    let mut done = 0usize;
    for _ in 0..MAX_DEPTH {
        let end = scene.entities.len();
        if done >= end {
            break;
        }
        expand_range(scene, prefabs, done, end, &mut report);
        let absorbed = flatten_children(scene);
        report.parts += scene.entities.len() + absorbed - end;
        done = end - absorbed;
    }
    report
}

/// How deep prefabs may nest (a prefab whose child is a prefab whose child
/// is ...): far past any real rig, and short of a cycle running forever.
const MAX_DEPTH: usize = 8;

fn expand_range(
    scene: &mut SceneData,
    prefabs: &HashMap<String, EntityData>,
    from: usize,
    to: usize,
    report: &mut ExpandReport,
) {
    for entity in &mut scene.entities[from..to] {
        let Some(instance) = entity.components.prefab_instance.clone() else {
            continue;
        };

        let Some(template) = prefabs.get(&instance.prefab_id) else {
            report.missing.push(instance.prefab_id.clone());
            continue;
        };

        // Deserialize back. If this fails the override JSON was
        // malformed; keep the entity as-is and surface via `missing`
        // so the runtime logs something useful.
        match merge_instance(&template.components, &entity.components, &instance) {
            Ok(mut bundle) => {
                // Always preserve the instance marker for tooling.
                bundle.prefab_instance = Some(instance.clone());
                entity.components = bundle;
                if entity.name.is_empty() {
                    entity.name = template.name.clone();
                }
                // The template's parts, then any the instance adds.
                let own = std::mem::take(&mut entity.children);
                entity.children = template.children.iter().cloned().chain(own).collect();
                report.expanded += 1;
            }
            Err(err) => {
                log::warn!(
                    "[prefab] failed to materialise instance of '{}': {}",
                    instance.prefab_id,
                    err
                );
                report.missing.push(instance.prefab_id.clone());
            }
        }
    }
}

/// Hang every entity's `children` out as entities of their own, `parent`ed
/// by name - which is how a v1 scene says hierarchy. A child is named
/// `parent/child` (and `#2`, `#3` for a second of the same name under one
/// parent), so two instances of one prefab never share a part's name;
/// `Entity.find_child` matches the part after the last `/`.
///
/// A scene can override one part: an entity it authors with the part's name
/// (`Cart/Wheel`) and its parent (`Cart`) is laid over that part, component
/// by component, and takes its place. Which is also why a scene saved with
/// its parts written out loads with each part once. Returns how many
/// authored entities were absorbed that way (they move to the end).
pub fn flatten_children(scene: &mut SceneData) -> usize {
    fn hang(parent: &str, children: Vec<EntityData>, out: &mut Vec<EntityData>) {
        let mut seen: HashMap<String, usize> = HashMap::new();
        for mut child in children {
            let n = seen.entry(child.name.clone()).or_insert(0);
            *n += 1;
            let own = if *n == 1 { child.name.clone() } else { format!("{}#{}", child.name, n) };
            child.name = format!("{parent}/{own}");
            child.parent = Some(parent.to_string());
            let grandchildren = std::mem::take(&mut child.children);
            let name = child.name.clone();
            out.push(child);
            hang(&name, grandchildren, out);
        }
    }
    // Parts find their parent by name, so a parent must have one of its
    // own: two instances left unnamed both take their prefab's, and every
    // wheel would hang from the first cart. A second one is renamed `#2`.
    let mut names: HashMap<String, usize> = HashMap::new();
    for i in 0..scene.entities.len() {
        let n = names.entry(scene.entities[i].name.clone()).or_insert(0);
        *n += 1;
        if *n > 1 && !scene.entities[i].children.is_empty() {
            let base = scene.entities[i].name.clone();
            let mut k = *n;
            while scene.entities.iter().any(|e| e.name == format!("{base}#{k}")) {
                k += 1;
            }
            log::info!("[prefab] a second '{base}' with parts is called '{base}#{k}'");
            scene.entities[i].name = format!("{base}#{k}");
        }
    }
    let mut out = Vec::new();
    for entity in &mut scene.entities {
        if !entity.children.is_empty() {
            let children = std::mem::take(&mut entity.children);
            hang(&entity.name, children, &mut out);
        }
    }
    if out.is_empty() {
        return 0;
    }
    let mut absorbed: Vec<usize> = Vec::new();
    for part in &mut out {
        let Some(i) = scene
            .entities
            .iter()
            .position(|e| e.name == part.name && e.parent == part.parent && e.children.is_empty())
        else {
            continue;
        };
        if absorbed.contains(&i) {
            continue;
        }
        // The same merge an instance gets: what the scene authored over
        // what the part is.
        let mut merged = bundle_to_value(&part.components);
        merge_object(&mut merged, &bundle_to_value(&scene.entities[i].components));
        match value_to_bundle(&merged) {
            Ok(bundle) => part.components = bundle,
            Err(err) => log::warn!("[prefab] override of part '{}' did not merge: {err}", part.name),
        }
        absorbed.push(i);
    }
    let n = absorbed.len();
    if n > 0 {
        let mut i = 0;
        scene.entities.retain(|_| {
            let keep = !absorbed.contains(&i);
            i += 1;
            keep
        });
    }
    scene.entities.extend(out);
    n
}

/// [`expand_prefab_instances`] for a v2 scene, on its JSON: a v2 entity's
/// components are a free map (package components among them), so the merge
/// is done on values - the template's components, the entity's over them,
/// then the overrides, as `merge_instance` does - and each part is hung out
/// as an entity of its own with a `parent_index`, so two instances of one
/// name keep their own parts. A scene-authored `children` list on a v2
/// entity is hung out the same way. An authored entity with a part's name
/// and parent is laid over that part in place. Until Oct 2026 a v2 scene
/// skipped `prefab_instance` entirely: the entity loaded with only what the
/// scene itself wrote.
fn expand_v2_instances(raw: &mut Value, prefabs: &HashMap<String, EntityData>) -> ExpandReport {
    let mut report = ExpandReport::default();
    let Some(entities) = raw.get_mut("entities").and_then(|e| e.as_array_mut()) else {
        return report;
    };
    let mut done = 0usize;
    for _ in 0..MAX_DEPTH {
        let end = entities.len();
        if done >= end {
            break;
        }
        let mut hangs: Vec<(usize, Vec<EntityData>)> = Vec::new();
        for (i, entity) in entities.iter_mut().enumerate().take(end).skip(done) {
            let mut parts: Vec<EntityData> = Vec::new();
            let instance = entity
                .get("components")
                .and_then(|c| c.get("prefab_instance"))
                .and_then(|v| serde_json::from_value::<crate::ecs::components::PrefabInstance>(v.clone()).ok());
            if let Some(instance) = instance {
                match prefabs.get(&instance.prefab_id) {
                    Some(template) => {
                        let mut merged = bundle_to_value(&template.components);
                        strip_nulls(&mut merged);
                        if let Some(own) = entity.get("components") {
                            merge_object(&mut merged, own);
                        }
                        for (path, value) in &instance.overrides {
                            set_by_path(&mut merged, path, value.clone());
                        }
                        entity["components"] = merged;
                        if entity.get("name").and_then(|n| n.as_str()).is_none_or(str::is_empty) {
                            entity["name"] = Value::String(template.name.clone());
                        }
                        parts.extend(template.children.iter().cloned());
                        report.expanded += 1;
                    }
                    None => report.missing.push(instance.prefab_id.clone()),
                }
            }
            if let Some(Value::Array(own)) = entity.as_object_mut().and_then(|o| o.remove("children")) {
                for child in own {
                    match serde_json::from_value::<EntityData>(child) {
                        Ok(c) => parts.push(c),
                        Err(err) => log::warn!("[prefab] a v2 child did not parse: {err}"),
                    }
                }
            }
            if !parts.is_empty() {
                hangs.push((i, parts));
            }
        }
        for (i, parts) in hangs {
            hang_v2(entities, i, parts, &mut report);
        }
        done = end;
    }
    report
}

/// Hang `parts` under `entities[parent]`, and theirs under them.
fn hang_v2(entities: &mut Vec<Value>, parent: usize, parts: Vec<EntityData>, report: &mut ExpandReport) {
    let parent_name = entities[parent].get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
    let mut seen: HashMap<String, usize> = HashMap::new();
    for mut part in parts {
        let n = seen.entry(part.name.clone()).or_insert(0);
        *n += 1;
        let own = if *n == 1 { part.name.clone() } else { format!("{}#{}", part.name, n) };
        let name = format!("{parent_name}/{own}");
        let grandchildren = std::mem::take(&mut part.children);
        let mut comps = bundle_to_value(&part.components);
        strip_nulls(&mut comps);
        // An authored entity with this part's name under this parent is
        // laid over it, in place.
        // Its parent is its `parent_index` when it has one (two carts may
        // share a name), else its `parent` by name.
        let authored = entities.iter().position(|e| {
            e.get("name").and_then(|n| n.as_str()) == Some(name.as_str())
                && match e.get("parent_index").and_then(|p| p.as_u64()) {
                    Some(p) => p == parent as u64,
                    None => e.get("parent").and_then(|p| p.as_str()) == Some(parent_name.as_str()),
                }
        });
        let at = match authored {
            Some(k) => {
                if let Some(over) = entities[k].get("components") {
                    merge_object(&mut comps, over);
                }
                entities[k]["components"] = comps;
                entities[k]["parent_index"] = Value::from(parent as u64);
                k
            }
            None => {
                entities.push(serde_json::json!({
                    "name": name, "parent": parent_name, "parent_index": parent, "components": comps,
                }));
                entities.len() - 1
            }
        };
        report.parts += 1;
        if !grandchildren.is_empty() {
            hang_v2(entities, at, grandchildren, report);
        }
    }
}

/// Drop the `null`s a serialized bundle carries for components it has not,
/// so a v2 entity gets no key it would read as a component.
fn strip_nulls(v: &mut Value) {
    if let Value::Object(m) = v {
        m.retain(|_, x| !x.is_null());
    }
}

/// A prefab's tree with every `prefab_instance` in it expanded - the node
/// merged over its prefab, the prefab's parts before its own - so a
/// runtime spawning it needs no prefab map. `resolve_prefab_variants` runs
/// it on every prefab of every map.
pub fn expand_node(node: &EntityData, prefabs: &HashMap<String, EntityData>, depth: usize) -> EntityData {
    let mut out = node.clone();
    if depth < MAX_DEPTH
        && let Some(instance) = node.components.prefab_instance.clone()
        && let Some(template) = prefabs.get(&instance.prefab_id)
    {
        match merge_instance(&template.components, &node.components, &instance) {
            Ok(mut bundle) => {
                bundle.prefab_instance = Some(instance.clone());
                out.components = bundle;
                out.children = template.children.iter().cloned().chain(node.children.iter().cloned()).collect();
            }
            Err(err) => log::warn!("[prefab] part '{}' of '{}' did not merge: {err}", node.name, instance.prefab_id),
        }
    }
    out.children = out.children.iter().map(|c| expand_node(c, prefabs, depth + 1)).collect();
    out
}

/// Spawn a prefab's parts under `parent`, each by `spawn_one` (the host's
/// way of turning `EntityData` into an entity) and given a `Parent`, their
/// own parts under them. The tree should be expanded (`expand_node`);
/// every runtime's prefab map is, by `resolve_prefab_variants`.
pub fn spawn_children(
    world: &mut hecs::World,
    parent: hecs::Entity,
    children: &[EntityData],
    spawn_one: &mut dyn FnMut(&mut hecs::World, &EntityData) -> hecs::Entity,
) {
    for child in children {
        let e = spawn_one(world, child);
        let _ = world.insert_one(e, crate::ecs::components::Parent(parent));
        spawn_children(world, e, &child.children, spawn_one);
    }
}

/// [`spawn_children`] through a component registry - the browsers' way,
/// which spawns every component by its scene key.
pub fn spawn_children_with_registry(
    world: &mut hecs::World,
    registry: &crate::package::component_registry::ComponentRegistry,
    parent: hecs::Entity,
    children: &[EntityData],
) {
    spawn_children(world, parent, children, &mut |w, data| {
        let e = crate::behavior::command::spawn_entity_data(w, registry, data);
        if w.get::<&crate::ecs::components::Transform>(e).is_err() {
            let _ = w.insert_one(e, crate::ecs::components::Transform::default());
        }
        e
    });
}

/// The merge itself: the template's bundle, the authored bundle over it
/// (authored wins), then the instance's dotted-path overrides.
fn merge_instance(
    template: &ComponentBundle,
    authored: &ComponentBundle,
    instance: &crate::ecs::components::PrefabInstance,
) -> Result<ComponentBundle, serde_json::Error> {
    let mut merged = bundle_to_value(template);
    merge_object(&mut merged, &bundle_to_value(authored));
    for (path, value) in &instance.overrides {
        set_by_path(&mut merged, path, value.clone());
    }
    value_to_bundle(&merged)
}

/// Resolve prefab **variants** (Oct 2026; Unity's Prefab Variants, Godot's
/// inherited scenes). A prefab whose own components carry a
/// `prefab_instance` extends the prefab it names: the base's components,
/// then the variant's own over them (its own win), then the instance's
/// dotted-path overrides - the same merge a scene instance gets, so a
/// variant reads exactly like an instance and a variant of a variant works.
///
/// ```json
/// { "name": "FireSlime", "components": {
///     "prefab_instance": { "prefab_id": "Slime", "overrides": { "sprite.color": [1, 0.4, 0.2, 1] } },
///     "point_light": { "radius": 120, "color": [1, 0.5, 0.2], "intensity": 1.5 } } }
/// ```
///
/// Every runtime calls this on its prefab map before anything spawns or
/// expands from it. A variant whose base is missing, or that is part of a
/// cycle, is left as authored and named in the log. The resolved prefab
/// keeps its `prefab_instance`, naming its base - what it came from.
pub fn resolve_prefab_variants(prefabs: &HashMap<String, EntityData>) -> HashMap<String, EntityData> {
    let mut out = HashMap::with_capacity(prefabs.len());
    add_prefabs(&mut out, prefabs);
    out
}

/// Add `raw` prefabs, as authored, to a map [`resolve_prefab_variants`]
/// already resolved - an asset pack's prefabs, arriving after the game's
/// (`asset::packs::PackContent`). A new variant may extend a prefab already
/// there; one of `raw`'s names replaces the prefab of that name. Resolving
/// the whole map again would merge each resolved variant over its base a
/// second time and give it its base's parts twice, which is why this
/// exists. A variant already in `resolved` whose base was missing stays as
/// it was - it is not looked at again.
pub fn add_prefabs(resolved: &mut HashMap<String, EntityData>, raw: &HashMap<String, EntityData>) {
    fn resolve(
        name: &str,
        prefabs: &HashMap<String, EntityData>,
        done: &mut HashMap<String, EntityData>,
        visiting: &mut Vec<String>,
    ) -> Option<EntityData> {
        if let Some(d) = done.get(name) {
            return Some(d.clone());
        }
        let raw = prefabs.get(name)?;
        let Some(instance) = raw.components.prefab_instance.clone() else {
            done.insert(name.to_string(), raw.clone());
            return Some(raw.clone());
        };
        if visiting.iter().any(|v| v == name) {
            log::warn!("[prefab] '{name}' extends itself ({} -> {name}); left as authored", visiting.join(" -> "));
            return Some(raw.clone());
        }
        visiting.push(name.to_string());
        let base = resolve(&instance.prefab_id, prefabs, done, visiting);
        visiting.pop();
        let out = match base {
            Some(base) => match merge_instance(&base.components, &raw.components, &instance) {
                Ok(mut bundle) => {
                    bundle.prefab_instance = Some(instance.clone());
                    // A variant keeps its base's parts and adds its own.
                    let children = base.children.iter().cloned().chain(raw.children.iter().cloned()).collect();
                    EntityData { components: bundle, children, ..raw.clone() }
                }
                Err(err) => {
                    log::warn!("[prefab] variant '{name}' of '{}' did not merge: {err}", instance.prefab_id);
                    raw.clone()
                }
            },
            None => {
                log::warn!("[prefab] variant '{name}' extends '{}', which is not loaded", instance.prefab_id);
                raw.clone()
            }
        };
        done.insert(name.to_string(), out.clone());
        Some(out)
    }
    // What is resolved already is done: a new variant's base is looked up
    // there before among the new ones.
    let mut done: HashMap<String, EntityData> =
        resolved.iter().filter(|(k, _)| !raw.contains_key(*k)).map(|(k, v)| (k.clone(), v.clone())).collect();
    for name in raw.keys() {
        if let Some(d) = resolve(name, raw, &mut done, &mut Vec::new()) {
            resolved.insert(name.clone(), d);
        }
    }
    // Then each new prefab's parts that are prefabs themselves, all the way
    // down, so a spawn needs no map.
    let all = resolved.clone();
    for name in raw.keys() {
        if let Some(d) = resolved.get_mut(name)
            && !d.children.is_empty()
        {
            d.children = d.children.iter().map(|c| expand_node(c, &all, 1)).collect();
        }
    }
}

/// A `.prefab` file's prefab and the name it is known by: its `name`, or
/// with none the file's stem (which it is then given). Every runtime keys
/// its prefab map this way.
pub fn parse_prefab(rel: &str, text: &str) -> Result<(String, EntityData), serde_json::Error> {
    let mut data: EntityData = serde_json::from_str(text)?;
    if data.name.is_empty() {
        data.name = std::path::Path::new(rel).file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string();
    }
    Ok((data.name.clone(), data))
}

/// Serialize a bundle to a JSON object (always succeeds — bundles are
/// fully serde-derived).
fn bundle_to_value(bundle: &ComponentBundle) -> Value {
    serde_json::to_value(bundle).unwrap_or_else(|_| Value::Object(Default::default()))
}

/// Deserialize a JSON object back into a bundle.
fn value_to_bundle(value: &Value) -> Result<ComponentBundle, serde_json::Error> {
    serde_json::from_value(value.clone())
}

/// Recursive object merge: every key in `overlay` is written into
/// `base`. Non-object values fully replace the base. Null values in
/// `overlay` are treated as "absent" so a scene-authored bundle with
/// `"sprite": null` (the serde default for `Option<SpriteComponent>`)
/// doesn't clobber the prefab's sprite.
fn merge_object(base: &mut Value, overlay: &Value) {
    let (Value::Object(base_obj), Value::Object(overlay_obj)) = (base, overlay) else {
        return;
    };
    for (key, overlay_val) in overlay_obj {
        if overlay_val.is_null() {
            continue;
        }
        match base_obj.get_mut(key) {
            Some(base_val) if base_val.is_object() && overlay_val.is_object() => {
                merge_object(base_val, overlay_val);
            }
            _ => {
                base_obj.insert(key.clone(), overlay_val.clone());
            }
        }
    }
}

/// Set a dotted-path value, creating intermediate objects as needed.
/// Numeric segments traverse arrays (`"path.points.0.x"`). Out-of-range
/// or type-mismatched segments are no-ops with a warn log.
fn set_by_path(root: &mut Value, path: &str, value: Value) {
    let mut segments = path.split('.').peekable();
    let mut cursor: &mut Value = root;

    while let Some(seg) = segments.next() {
        let is_last = segments.peek().is_none();
        if is_last {
            assign_segment(cursor, seg, value);
            return;
        }
        cursor = match descend_segment(cursor, seg) {
            Some(next) => next,
            None => {
                log::warn!("[prefab] override path '{path}' could not descend at '{seg}'");
                return;
            }
        };
    }
}

fn descend_segment<'a>(cursor: &'a mut Value, seg: &str) -> Option<&'a mut Value> {
    if let Ok(idx) = seg.parse::<usize>()
        && let Value::Array(arr) = cursor
    {
        return arr.get_mut(idx);
    }
    if !cursor.is_object() {
        return None;
    }
    let Value::Object(obj) = cursor else {
        return None;
    };
    // Vivify missing keys as empty objects so deep overrides work even
    // when the prefab didn't author the parent path.
    if !obj.contains_key(seg) {
        obj.insert(seg.to_string(), Value::Object(Default::default()));
    }
    obj.get_mut(seg)
}

fn assign_segment(cursor: &mut Value, seg: &str, value: Value) {
    if let Ok(idx) = seg.parse::<usize>()
        && let Value::Array(arr) = cursor
    {
        if idx < arr.len() {
            arr[idx] = value;
        } else {
            log::warn!("[prefab] override array index {idx} out of range");
        }
        return;
    }
    if let Value::Object(obj) = cursor {
        obj.insert(seg.to_string(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::components::{PrefabInstance, SpriteComponent, Transform};
    use glam::Vec2;
    use serde_json::json;

    fn make_player_template() -> EntityData {
        EntityData {
            children: Vec::new(),
            name: "Player".to_string(),
            parent: None,
            components: ComponentBundle {
                transform: Some(Transform {
                    position: Vec2::new(0.0, 0.0),
                    rotation: 0.0,
                    scale: Vec2::ONE,
                }),
                sprite: Some(SpriteComponent {
                    texture_id: "player.png".into(),
                    color: [1.0, 1.0, 1.0, 1.0],
                    z_order: 0,
                    ..Default::default()
                }),
                ..Default::default()
            },
        }
    }

    fn make_instance_entity(prefab_id: &str, overrides: HashMap<String, Value>) -> EntityData {
        EntityData {
            children: Vec::new(),
            name: String::new(),
            parent: None,
            components: ComponentBundle {
                prefab_instance: Some(PrefabInstance {
                    prefab_id: prefab_id.to_string(),
                    overrides,
                }),
                ..Default::default()
            },
        }
    }

    #[test]
    fn expand_clones_template_into_instance() {
        let mut prefabs = HashMap::new();
        prefabs.insert("Player".to_string(), make_player_template());

        let mut scene = SceneData {
            name: "Test".to_string(),
            entities: vec![make_instance_entity("Player", HashMap::new())],
            ..Default::default()
        };

        let report = expand_prefab_instances(&mut scene, &prefabs);
        assert_eq!(report.expanded, 1);
        assert!(report.missing.is_empty());

        let bundle = &scene.entities[0].components;
        assert_eq!(bundle.sprite.as_ref().unwrap().texture_id, "player.png");
        assert!(bundle.transform.is_some());
        assert_eq!(scene.entities[0].name, "Player");
        // Marker is preserved.
        assert!(bundle.prefab_instance.is_some());
    }

    #[test]
    fn dotted_path_override_patches_field() {
        let mut prefabs = HashMap::new();
        prefabs.insert("Player".to_string(), make_player_template());

        let mut overrides = HashMap::new();
        overrides.insert("sprite.color".to_string(), json!([1.0, 0.0, 0.0, 1.0]));
        overrides.insert("transform.position".to_string(), json!([100.0, 200.0]));

        let mut scene = SceneData {
            name: "Test".to_string(),
            entities: vec![make_instance_entity("Player", overrides)],
            ..Default::default()
        };

        expand_prefab_instances(&mut scene, &prefabs);

        let bundle = &scene.entities[0].components;
        assert_eq!(bundle.sprite.as_ref().unwrap().color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(
            bundle.transform.as_ref().unwrap().position,
            Vec2::new(100.0, 200.0)
        );
        // Other prefab fields preserved.
        assert_eq!(bundle.sprite.as_ref().unwrap().texture_id, "player.png");
    }

    #[test]
    fn whole_component_override_replaces_template() {
        let mut prefabs = HashMap::new();
        prefabs.insert("Player".to_string(), make_player_template());

        let mut overrides = HashMap::new();
        overrides.insert(
            "sprite".to_string(),
            json!({
                "texture_id": "enemy.png",
                "color": [0.0, 1.0, 0.0, 1.0],
                "z_order": 5,
                "size": [32.0, 32.0]
            }),
        );

        let mut scene = SceneData {
            name: "Test".to_string(),
            entities: vec![make_instance_entity("Player", overrides)],
            ..Default::default()
        };

        expand_prefab_instances(&mut scene, &prefabs);

        let sprite = scene.entities[0].components.sprite.as_ref().unwrap();
        assert_eq!(sprite.texture_id, "enemy.png");
        assert_eq!(sprite.z_order, 5);
        assert_eq!(sprite.color, [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn authored_component_wins_over_template() {
        let mut prefabs = HashMap::new();
        prefabs.insert("Player".to_string(), make_player_template());

        // Authored entity already has its own transform; expander
        // should keep it instead of the prefab's origin transform.
        let mut entity = make_instance_entity("Player", HashMap::new());
        entity.components.transform = Some(Transform {
            position: Vec2::new(50.0, 60.0),
            rotation: 1.0,
            scale: Vec2::splat(2.0),
        });
        let mut scene = SceneData {
            name: "Test".to_string(),
            entities: vec![entity],
            ..Default::default()
        };

        expand_prefab_instances(&mut scene, &prefabs);

        let t = scene.entities[0].components.transform.as_ref().unwrap();
        assert_eq!(t.position, Vec2::new(50.0, 60.0));
        assert_eq!(t.rotation, 1.0);
        // Sprite still came from the template since it wasn't authored.
        assert!(scene.entities[0].components.sprite.is_some());
    }

    #[test]
    fn missing_prefab_reports_id_and_leaves_marker() {
        let prefabs: HashMap<String, EntityData> = HashMap::new();

        let mut scene = SceneData {
            name: "Test".to_string(),
            entities: vec![make_instance_entity("Ghost", HashMap::new())],
            ..Default::default()
        };

        let report = expand_prefab_instances(&mut scene, &prefabs);
        assert_eq!(report.expanded, 0);
        assert_eq!(report.missing, vec!["Ghost".to_string()]);
        // Marker stays so tooling can flag the dangling reference.
        assert!(scene.entities[0].components.prefab_instance.is_some());
        assert!(scene.entities[0].components.sprite.is_none());
    }

    #[test]
    fn instance_without_marker_is_untouched() {
        let prefabs: HashMap<String, EntityData> = HashMap::new();
        let mut scene = SceneData {
            name: "Test".to_string(),
            entities: vec![EntityData {
                children: Vec::new(),
                name: "Plain".to_string(),
                parent: None,
                components: ComponentBundle::default(),
            }],
            ..Default::default()
        };

        let report = expand_prefab_instances(&mut scene, &prefabs);
        assert_eq!(report.expanded, 0);
        assert!(report.missing.is_empty());
    }

    #[test]
    fn vivifies_missing_parent_path() {
        let mut prefabs = HashMap::new();
        let mut template = make_player_template();
        // Strip the transform so the override has to vivify it.
        template.components.transform = None;
        prefabs.insert("Player".to_string(), template);

        let mut overrides = HashMap::new();
        overrides.insert("transform.position".to_string(), json!([10.0, 20.0]));

        let mut scene = SceneData {
            name: "Test".to_string(),
            entities: vec![make_instance_entity("Player", overrides)],
            ..Default::default()
        };

        expand_prefab_instances(&mut scene, &prefabs);

        // Vivified parent path won't pass strict deserialize (Transform
        // needs rotation/scale) — the expander logs a warn and routes
        // through `missing`. Verify graceful handling rather than a
        // panic.
        let bundle = &scene.entities[0].components;
        // Marker preserved either way.
        assert!(bundle.prefab_instance.is_some());
    }
}
