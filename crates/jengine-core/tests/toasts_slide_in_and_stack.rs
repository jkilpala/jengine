//! **Toasts** (Oct 2026; `ui::toast`): notices that slide in from a corner,
//! stack and slide away. Driven as a host does - `Toast.show` through the
//! shared host command a browser script reaches, the frame through
//! `update_ui_widgets` - two toasts come up in the top-right corner, the
//! newest nearest it, each a card with its icon, title and wrapped text
//! drawn as entity UI, and the stack's script is told of each. The timing
//! (slide, stay, leave, queue) runs on a clock the module's own tests drive.
//! Found by the showcase's Trophy Room.
#![cfg(feature = "toast")]

use glam::Vec2;
use hecs::{Entity, World};
use jengine_core::input::InputState;
use jengine_core::scripting::host_commands::{JsonArgs, ScriptWorldHost, handle_world_command};
use jengine_core::ui::toast::{ToastPart, ToastStack};
use jengine_core::ui::{UIElement, UIImage, UIText};

struct Host(World);

impl ScriptWorldHost for Host {
    fn world(&mut self) -> &mut World {
        &mut self.0
    }
    fn resolve_entity(&mut self, id: u32) -> Option<Entity> {
        self.0.iter().map(|e| e.entity()).find(|e| e.id() == id)
    }
}

fn frame(world: &mut World) {
    let mut input = InputState::new();
    let mut clicked = Vec::new();
    jengine_core::ecs::systems::update_ui_widgets(world, &mut input, (0.0, 0.0), 1280.0, 720.0, &mut clicked);
}

#[test]
fn two_toasts_come_up_in_the_corner_newest_on_top() {
    let mut world = World::new();
    let stack = world.spawn((ToastStack { slide: 0.0001, ..Default::default() },));
    let mut host = Host(world);
    for (title, text) in [("First Steps", "Walked a hundred paces."), ("Collector", "Found every one of the seven hidden shells on the beach.")] {
        let args = JsonArgs(serde_json::json!({ "e": stack.id(), "n": title, "x": text, "i": "icons/shell.png", "s": 5.0 }));
        assert!(handle_world_command(&mut host, "tsho", &args), "a shared command");
    }
    let mut world = host.0;
    let _ = jengine_core::ecs::callbacks::take();
    frame(&mut world);
    // Slid in by the next frame (a slide of a tenth of a millisecond).
    std::thread::sleep(std::time::Duration::from_millis(5));
    frame(&mut world);
    assert_eq!(world.query::<&ToastPart>().iter().count(), 8, "two cards, each with an icon, a title and its text");
    let titles: Vec<(String, Vec2)> = world
        .query::<(&UIText, &UIElement, &ToastPart)>()
        .iter()
        .filter(|(_, (t, _, _))| t.font_size == ToastStack::default().title_size)
        .map(|(_, (t, el, _))| (t.text.clone(), el.offset))
        .collect();
    let at = |name: &str| titles.iter().find(|(t, _)| t == name).map(|(_, p)| *p).expect(name);
    let (first, second) = (at("First Steps"), at("Collector"));
    assert!(second.y > first.y, "the newest nearest the top corner: {second} over {first}");
    assert!(first.x > 1280.0 - 400.0 && first.x < 1280.0, "in against the right edge: {first}");
    let wrapped = world
        .query::<(&UIText, &ToastPart)>()
        .iter()
        .map(|(_, (t, _))| t.text.clone())
        .find(|t| t.starts_with("Found"))
        .unwrap();
    assert!(wrapped.contains('\n'), "the long text wraps: {wrapped:?}");
    let icons = world.query::<(&UIImage, &ToastPart)>().iter().filter(|(_, (i, _))| i.texture_id.as_str() == "icons/shell.png").count();
    assert_eq!(icons, 2, "each with its icon");
    let told = jengine_core::ecs::callbacks::take().into_iter().filter(|c| c.handler == "on_toast" && c.entity == stack).count();
    assert_eq!(told, 2, "the stack's script told of each");
}
