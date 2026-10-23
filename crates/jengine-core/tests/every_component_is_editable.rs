//! Every component the engine registers can be added and edited in the
//! editor: either the Inspector has a hand-written section for it (the
//! `KNOWN_COMPONENTS` list in `editor/src/lib/componentDefs.ts`), or its
//! inspector schema is derived from the component itself
//! (`editor_registry::derived_component_info`).
//!
//! Until Sep 2026 neither held for anything added after the hand-kept list
//! stopped growing: `trail`, `toon_lighting`, `directional_light`,
//! `ambient_occlusion`, `mesh` and a dozen more showed in the Inspector as
//! "No schema - raw data attached" and were missing from Add Component.

use jengine_core::package::component_registry::ComponentRegistry;
use jengine_core::package::core_registration::register_core_components;
use jengine_core::package::editor_registry::{FieldType, derived_component_info};

/// Components that cannot be built from `{}` (a required field) and have no
/// hand-written section. The list may only shrink.
const NOT_DERIVABLE: &[&str] = &[];

fn known_in_editor() -> Vec<String> {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../editor/src/lib/componentDefs.ts"))
        .expect("componentDefs.ts");
    let start = src.find("KNOWN_COMPONENTS").expect("the list");
    let body = &src[start..src[start..].find("];").map(|i| start + i).expect("its end")];
    body.split('"').skip(1).step_by(2).map(str::to_string).collect()
}

#[test]
fn every_registered_component_can_be_edited() {
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    let known = known_in_editor();
    assert!(known.contains(&"transform".to_string()), "parsed the editor's list: {known:?}");
    let mut missing = Vec::new();
    let mut listing = Vec::new();
    for desc in registry.iter() {
        if known.contains(&desc.name) {
            continue;
        }
        match derived_component_info(desc) {
            Some(info) => {
                let fields: Vec<String> = info.fields.iter().map(|f| format!("{}:{:?}", f.name, f.field_type)).collect();
                listing.push(format!("{} -> {}", desc.name, fields.join(", ")));
            }
            None if NOT_DERIVABLE.contains(&desc.name.as_str()) => {}
            None => missing.push(desc.name.clone()),
        }
    }
    listing.sort();
    eprintln!("{}", listing.join("\n"));
    assert!(missing.is_empty(), "components the editor cannot add or edit: {missing:?}");
    for name in NOT_DERIVABLE {
        assert!(
            derived_component_info(registry.get(name).expect("still registered")).is_none(),
            "{name} derives now - take it off NOT_DERIVABLE"
        );
    }
}

#[test]
fn enums_become_dropdowns_and_colours_pickers() {
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    let info = derived_component_info(registry.get("trail").expect("trail")).expect("derives");
    let field = |n: &str| info.fields.iter().find(|f| f.name == n).unwrap_or_else(|| panic!("{n} in {:?}", info.fields));
    match &field("blend_mode").field_type {
        FieldType::Enum { options } => assert!(options.iter().any(|o| o == "Additive"), "{options:?}"),
        other => panic!("blend_mode is a dropdown, not {other:?}"),
    }
    assert!(matches!(field("color").field_type, FieldType::Color));
    assert!(matches!(field("texture_id").field_type, FieldType::AssetRef { .. }));
    assert!(matches!(field("emitting").field_type, FieldType::Bool));
    let sun = derived_component_info(registry.get("directional_light").expect("sun")).expect("derives");
    assert!(sun.fields.iter().any(|f| f.name == "color" && matches!(f.field_type, FieldType::Color3)));
    // A list of points (a spline shape's) is a list the inspector can edit -
    // it used to derive to nothing, so a curve could not be changed at all.
    let spline = derived_component_info(registry.get("spline_shape").expect("spline")).expect("derives");
    let points = spline.fields.iter().find(|f| f.name == "points").expect("points is offered");
    assert!(matches!(points.field_type, FieldType::Points), "{:?}", points.field_type);
    assert!(spline.fields.iter().any(|f| f.name == "fill_texture" && matches!(f.field_type, FieldType::AssetRef { .. })));
}

/// A line's curves are graphs and its points a list, though all three are
/// empty by default: an empty list derived to nothing, so none of them
/// could be edited in the Inspector at all.
#[test]
fn curves_and_gradients_get_their_widgets() {
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    let line = derived_component_info(registry.get("line").expect("line")).expect("derives");
    let kind = |n: &str| {
        &line.fields.iter().find(|f| f.name == n).unwrap_or_else(|| panic!("{n} in {:?}", line.fields)).field_type
    };
    assert!(matches!(kind("width_curve"), FieldType::Curve), "{:?}", kind("width_curve"));
    assert!(matches!(kind("gradient"), FieldType::Gradient), "{:?}", kind("gradient"));
    assert!(matches!(kind("points"), FieldType::Points), "{:?}", kind("points"));
}

fn editor_file(name: &str) -> String {
    std::fs::read_to_string(format!("{}/../../editor/src/lib/{name}", env!("CARGO_MANIFEST_DIR")))
        .unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// `name: ["a", "b"],` rows of the `export const <list>` record in
/// `componentDefs.ts`.
fn field_lists(defs: &str, list: &str) -> Vec<(String, Vec<String>)> {
    let start = defs.find(&format!("export const {list}")).unwrap_or_else(|| panic!("{list} in componentDefs.ts"));
    let body = &defs[start..];
    let body = &body[body.find('{').expect("its body") + 1..body.find("\n};").expect("its end")];
    body.lines()
        .filter_map(|line| {
            let line = line.trim();
            let (name, rest) = line.split_once(':')?;
            if name.starts_with("//") {
                return None;
            }
            let fields = rest.split('"').skip(1).step_by(2).map(str::to_string).collect();
            Some((name.trim().to_string(), fields))
        })
        .collect()
}

/// Whether `src` reads `<prefix><field>`, `field` ending at a word boundary.
fn mentions(src: &str, prefix: &str, field: &str) -> bool {
    let needle = format!("{prefix}{field}");
    src.match_indices(&needle).any(|(i, _)| {
        !src[i + needle.len()..].chars().next().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

/// Check one inspector's hand-written sections: every field of each section's
/// component is read through `<root>.<comp>.` (or an `{@const x = <root>.<comp>}`
/// alias, or `extra`'s own reads), or is listed in `undrawn_list` (drawn under
/// the section by the derived widgets) or `NOT_DRAWN_FIELDS`. `comps` is
/// which components the file has a section for.
fn check_sections(file: &str, root: &str, comps: &[String], undrawn_list: &str, extra: &[(&str, String, &str)]) {
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    let src = editor_file(file);
    let defs = editor_file("componentDefs.ts");
    let undrawn = field_lists(&defs, undrawn_list);
    let not_drawn = field_lists(&defs, "NOT_DRAWN_FIELDS");
    let listed = |lists: &[(String, Vec<String>)], comp: &str, field: &str| {
        lists.iter().any(|(c, f)| c == comp && f.iter().any(|x| x == field))
    };
    // `{@const pe = entity.components.particle_emitter}` - a section's alias.
    let mut aliases: Vec<(String, String)> = Vec::new();
    let assign = format!(" = {root}.");
    for (i, _) in src.match_indices("{@const ") {
        let rest = &src[i + 8..];
        let Some((alias, rest)) = rest.split_once(&assign) else { continue };
        let comp: String = rest.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if alias.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            aliases.push((comp, alias.to_string()));
        }
    }

    let mut missing = Vec::new();
    let mut twice = Vec::new();
    let mut stale = Vec::new();
    for comp in comps {
        let Some(desc) = registry.get(comp) else { continue };
        // The derived schema's fields: serde's list with aliases left out.
        let fields: Vec<String> = match derived_component_info(desc) {
            Some(info) => info.fields.into_iter().map(|f| f.name).collect(),
            None => (desc.fields_fn)().unwrap_or(&[]).iter().map(|f| f.to_string()).collect(),
        };
        for field in &fields {
            let mut prefixes = vec![format!("{root}.{comp}."), format!("{root}.{comp}?.")];
            for (c, alias) in &aliases {
                if c == comp {
                    prefixes.push(format!("{alias}."));
                    prefixes.push(format!("{alias}?."));
                }
            }
            let drawn = prefixes.iter().any(|p| mentions(&src, p, field))
                || extra.iter().any(|(c, other, name)| {
                    c == comp && (mentions(other, &format!("{name}."), field) || mentions(other, &format!("{name}?."), field))
                });
            let in_undrawn = listed(&undrawn, comp, field);
            if drawn && in_undrawn {
                twice.push(format!("{comp}.{field}"));
            } else if !drawn && !in_undrawn && !listed(&not_drawn, comp, field) {
                missing.push(format!("{comp}.{field}"));
            }
        }
        for (c, list) in undrawn.iter().chain(not_drawn.iter()) {
            if c == comp {
                stale.extend(list.iter().filter(|f| !fields.contains(f)).map(|f| format!("{comp}.{f}")));
            }
        }
    }
    assert!(
        missing.is_empty(),
        "{file}: fields no section draws - draw them, or list them in componentDefs.ts \
         {undrawn_list} (drawn generically) or NOT_DRAWN_FIELDS: {missing:?}"
    );
    assert!(twice.is_empty(), "{file}: drawn by their section and listed in {undrawn_list} too: {twice:?}");
    assert!(stale.is_empty(), "{file}: listed in componentDefs.ts but not fields of the component: {stale:?}");
    for (comp, _) in &undrawn {
        assert!(comps.contains(comp), "{undrawn_list} lists {comp}, which has no section in {file}");
        assert!(
            src.contains(&format!("undrawnFields(\"{comp}\")")),
            "{comp} has {undrawn_list} but its section in {file} does not render them"
        );
    }
}

/// A hand-written Inspector section draws every field of its component, or
/// `componentDefs.ts` says it does not: `UNDRAWN_FIELDS` are drawn under the
/// section by the derived widgets, `NOT_DRAWN_FIELDS` (state the engine keeps
/// while playing, or a field its section sets some way this cannot see) by
/// nothing. Before this, a field added to a component with a hand-written
/// section was simply missing from the editor - the lights' `casts_shadows`
/// and `height`, `text.align`, `sprite.fill`, `camera.confine`,
/// `collider.one_way` and some forty more.
#[test]
fn hand_written_sections_draw_every_field() {
    // The particle emitter's newer fields are drawn by its own panel, which
    // is handed the component as `emitter`.
    let extras = editor_file("ParticleExtras.svelte");
    check_sections(
        "Inspector.svelte",
        "entity.components",
        &known_in_editor(),
        "UNDRAWN_FIELDS",
        &[("particle_emitter", extras, "emitter")],
    );
}

/// The prefab inspector's few hand-written sections, the same way; every
/// other component it shows whole through the derived widgets.
#[test]
fn prefab_sections_draw_every_field() {
    let src = editor_file("PrefabInspector.svelte");
    let comps: Vec<String> = src
        .match_indices("{#if !collapsed[\"")
        .map(|(i, _)| src[i + 17..].split('"').next().unwrap_or_default().to_string())
        .collect();
    assert!(comps.contains(&"sprite".to_string()), "found its sections: {comps:?}");
    check_sections("PrefabInspector.svelte", "data.components", &comps, "PREFAB_UNDRAWN_FIELDS", &[]);
}

/// Fields a default leaves out (`skip_serializing_if`) still derive, typed by
/// what serde keeps there, and a field with no widget of its own is JSON
/// text rather than missing.
#[test]
fn skipped_and_shapeless_fields_derive_too() {
    let mut registry = ComponentRegistry::new();
    register_core_components(&mut registry);
    let kind = |comp: &str, field: &str| {
        let info = derived_component_info(registry.get(comp).expect(comp)).expect("derives");
        info.fields
            .iter()
            .find(|f| f.name == field)
            .unwrap_or_else(|| panic!("{comp}.{field} in {:?}", info.fields))
            .field_type
            .clone()
    };
    // An enum whose default is skipped when saved.
    match kind("text", "align") {
        FieldType::Enum { options } => assert!(options.iter().any(|o| o.eq_ignore_ascii_case("center")), "{options:?}"),
        other => panic!("text.align is a dropdown, not {other:?}"),
    }
    // A struct, which serde would also read from an array, is not points.
    assert!(matches!(kind("ui_text", "shadow"), FieldType::Json), "{:?}", kind("ui_text", "shadow"));
    assert!(matches!(kind("camera", "confine"), FieldType::Points), "{:?}", kind("camera", "confine"));
    assert!(matches!(kind("text", "text_key"), FieldType::String));
    assert!(matches!(kind("joint", "break_force"), FieldType::Float { .. }));
    // Maps and nested trees: text, not absent.
    assert!(matches!(kind("timeline", "clips"), FieldType::Json));
    assert!(matches!(kind("behavior_tree", "root"), FieldType::Json));
}
