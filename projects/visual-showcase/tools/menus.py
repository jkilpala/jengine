"""Menus & HUD (showcase scene 22): three `.gui` documents built in code.

- `ui/mh_menu.gui` - a title card: pill buttons with shadows that ease into
  their hover colour and grow a little (`hover_scale`), centred labels, the
  arcade's 64 px font for the title;
- `ui/mh_inventory.gui` - a grid of rounded slots holding item icons, and a
  details panel with rounded stat bars;
- `ui/mh_hud.gui` - health and mana bars, a gold counter, an ability bar
  with cooldowns, and a toast that slides in.

Every screen's outermost card is what the script fades (`opacity`) and pops
(`scale`) on a change of screen: both are inherited, so one number moves the
whole card and everything on it.
"""

import json

FONT = "fonts/tabletop.font"
ICONS = "sprites/env/mh_icons.png"
N_ICONS = 8
# name, icon, attack, defence, weight, description
ITEMS = [
    ("Ember Blade", 0, 0.82, 0.10, 0.45, "Forged in the last kiln."),
    ("Warden's Shield", 1, 0.08, 0.88, 0.70, "It has stopped worse."),
    ("Draught of Vigour", 2, 0.0, 0.0, 0.05, "Restores 60 health."),
    ("Draught of Clarity", 3, 0.0, 0.0, 0.05, "Restores 40 mana."),
    ("Heartstone", 4, 0.30, 0.30, 0.10, "Warm to the touch."),
    ("Ash Longbow", 5, 0.66, 0.05, 0.30, "Quiet as falling ash."),
    ("Map of the Reach", 6, 0.0, 0.0, 0.02, "Half of it is guesses."),
    ("Old Crown", 7, 0.0, 0.12, 0.15, "Someone will want this."),
]
SLOTS = 20


def lin(r, g, b, a=1.0):
    def c(v):
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4
    return [round(c(r), 5), round(c(g), 5), round(c(b), 5), a]


INK = lin(0.95, 0.93, 0.88)
DIM = lin(0.66, 0.66, 0.72)
EMBER = lin(1.0, 0.62, 0.25)
CARD = lin(0.07, 0.08, 0.11, 0.94)
SHADOW = {"color": [0.0, 0.0, 0.0, 0.55], "offset": [0.0, 10.0], "blur": 22.0}


def fixed(v):
    return {"type": "Fixed", "value": float(v)}


def pct(v):
    return {"type": "Percent", "value": float(v)}


def node(nid, node_type, children=(), **style):
    st = {"width": style.pop("width", {"type": "Auto"}), "height": style.pop("height", {"type": "Auto"})}
    st.update(style)
    return {"id": nid, "node_type": node_type, "style": st, "children": list(children)}


def text(nid, s, size, color, w, h, align="Center", font=FONT):
    return node(nid, {"type": "Text", "text": s, "font_id": font, "font_size": float(size), "color": color,
                      "alignment": align}, width=fixed(w), height=fixed(h))


def flex(direction, gap=0.0, align="Center", justify="Center"):
    return {"direction": direction, "gap": float(gap), "align_items": align, "justify_content": justify}


def pad(v):
    return {"top": float(v), "right": float(v), "bottom": float(v), "left": float(v)}


def button(nid, label, w, h, size=26, radius=None, normal=None, hover=None):
    return node(nid, {"type": "Button", "label": label, "font_id": FONT, "font_size": float(size),
                      "text_color": INK, "normal_color": normal or lin(0.17, 0.19, 0.26),
                      "hover_color": hover or EMBER, "pressed_color": lin(0.75, 0.40, 0.12),
                      "alignment": "Center", "hover_scale": 1.06},
                width=fixed(w), height=fixed(h), corner_radius=float(radius if radius is not None else h / 2),
                shadow={"color": [0.0, 0.0, 0.0, 0.45], "offset": [0.0, 5.0], "blur": 10.0})


def icon_uv(i):
    return [i / N_ICONS, 0.0, (i + 1) / N_ICONS, 1.0]


def doc(name, root):
    return {"name": name, "anchor": "TopLeft", "root": root}


def screen(card):
    """A full-screen root that centres `card`."""
    return node("root", {"type": "Panel"}, [card], width=pct(100), height=pct(100),
                flex=flex("Column", 0, "Center", "Center"))


def menu_doc():
    buttons = [button(f"btn_{k}", label, 300, 56) for k, label in
               (("play", "Play"), ("inventory", "Inventory"), ("settings", "Settings"), ("quit", "Quit"))]
    card = node("card", {"type": "Panel"}, [
        text("title", "EMBERFALL", 64, EMBER, 380, 74),
        text("subtitle", "a jEngine showcase", 22, DIM, 380, 30),
        node("spacer", {"type": "Panel"}, width=fixed(10), height=fixed(8)),
        *buttons,
        text("hint", "hover, click - or wait", 18, DIM, 380, 26),
    ], width=fixed(440), height=fixed(500), bg_color=CARD, corner_radius=26.0, shadow=SHADOW,
        padding=pad(24), flex=flex("Column", 14, "Center", "Start"))
    return doc("Menu", screen(card))


def inventory_doc():
    slots = []
    for i in range(SLOTS):
        children = []
        if i < len(ITEMS):
            children.append(node(f"slot_icon_{i}", {"type": "Image", "texture_id": ICONS, "tint": [1, 1, 1, 1],
                                                     "uv_rect": icon_uv(ITEMS[i][1])},
                                 width=fixed(58), height=fixed(58)))
        s = node(f"slot_{i}", {"type": "Button", "label": "", "font_id": FONT, "font_size": 12.0,
                               "text_color": INK, "normal_color": lin(0.13, 0.14, 0.19),
                               "hover_color": lin(0.30, 0.25, 0.20), "pressed_color": lin(0.45, 0.30, 0.15),
                               "hover_scale": 1.08},
                 children, width=fixed(80), height=fixed(80), corner_radius=14.0,
                 border_color=[0, 0, 0, 0], border_width=3.0,
                 flex=flex("Column", 0, "Center", "Center"))
        slots.append(s)
    grid = node("grid", {"type": "Panel"}, slots, width=fixed(470), height=fixed(380),
                layout={"mode": "Grid", "columns": [fixed(80)] * 5, "rows": [fixed(80)] * 4,
                        "column_gap": 16.0, "row_gap": 16.0})

    def stat(key, label, color):
        return node(f"row_{key}", {"type": "Panel"}, [
            text(f"label_{key}", label, 20, DIM, 100, 26, "Left"),
            node(f"bar_{key}", {"type": "ProgressBar", "value": 0.5, "min": 0.0, "max": 1.0, "fg_color": color,
                                "bg_color": lin(0.16, 0.17, 0.22)},
                 width=fixed(190), height=fixed(14), corner_radius=7.0),
        ], width=fixed(300), height=fixed(28), flex=flex("Row", 10, "Center", "Start"))

    details = node("details", {"type": "Panel"}, [
        node("item_frame", {"type": "Panel"}, [
            node("item_icon", {"type": "Image", "texture_id": ICONS, "tint": [1, 1, 1, 1], "uv_rect": icon_uv(0)},
                 width=fixed(96), height=fixed(96)),
        ], width=fixed(128), height=fixed(128), bg_color=lin(0.13, 0.14, 0.19), corner_radius=22.0,
            shadow={"color": [0, 0, 0, 0.4], "offset": [0, 6], "blur": 14}, flex=flex("Column", 0, "Center", "Center")),
        text("item_name", ITEMS[0][0], 30, INK, 300, 40),
        text("item_desc", ITEMS[0][5], 18, DIM, 300, 26),
        stat("atk", "Attack", lin(0.95, 0.40, 0.25)),
        stat("def", "Defence", lin(0.35, 0.60, 0.95)),
        stat("wgt", "Weight", lin(0.80, 0.75, 0.45)),
        node("buttons", {"type": "Panel"}, [
            button("btn_equip", "Equip", 140, 46, 22),
            button("btn_back", "Back", 140, 46, 22, normal=lin(0.22, 0.14, 0.14)),
        ], width=fixed(300), height=fixed(56), flex=flex("Row", 16, "Center", "Center")),
    ], width=fixed(340), height=fixed(472), bg_color=lin(0.10, 0.11, 0.15, 0.9), corner_radius=18.0,
        padding=pad(18), flex=flex("Column", 10, "Center", "Start"))
    left = node("left", {"type": "Panel"}, [
        node("inv_head", {"type": "Panel"}, [
            text("inv_title", "INVENTORY", 40, EMBER, 260, 50, "Left"),
            node("gold", {"type": "Panel"}, [
                node("gold_icon", {"type": "Image", "texture_id": ICONS, "tint": [1, 1, 1, 1], "uv_rect": icon_uv(7)},
                     width=fixed(30), height=fixed(30)),
                text("gold_text", "1 240", 24, lin(1.0, 0.85, 0.35), 110, 30, "Left"),
            ], width=fixed(160), height=fixed(40), flex=flex("Row", 6, "Center", "End")),
        ], width=fixed(470), height=fixed(56), flex=flex("Row", 0, "Center", "SpaceBetween")),
        grid,
    ], width=fixed(470), height=fixed(472), flex=flex("Column", 16, "Start", "Start"))
    card = node("card", {"type": "Panel"}, [left, details], width=fixed(900), height=fixed(520), bg_color=CARD,
                corner_radius=24.0, shadow=SHADOW, padding=pad(24), flex=flex("Row", 22, "Start", "Start"))
    return doc("Inventory", screen(card))


def hud_doc():
    def bar(nid, color, w, h):
        return node(nid, {"type": "ProgressBar", "value": 1.0, "min": 0.0, "max": 1.0, "fg_color": color,
                          "bg_color": lin(0.10, 0.10, 0.13, 0.85)},
                    width=fixed(w), height=fixed(h), corner_radius=h / 2)
    status = node("status", {"type": "Panel"}, [
        node("portrait", {"type": "Circle", "fill_color": lin(0.55, 0.30, 0.18), "stroke_color": EMBER,
                          "stroke_width": 3.0}, width=fixed(72), height=fixed(72)),
        node("bars", {"type": "Panel"}, [
            text("hero_name", "Wren  Lv 7", 22, INK, 240, 26, "Left"),
            bar("hp", lin(0.90, 0.25, 0.22), 240, 18),
            bar("mp", lin(0.30, 0.55, 0.98), 200, 12),
            bar("xp", lin(0.95, 0.80, 0.30), 240, 6),
        ], width=fixed(250), height=fixed(80), flex=flex("Column", 6, "Start", "Center")),
    ], width=fixed(360), height=fixed(100), bg_color=lin(0.06, 0.07, 0.10, 0.72), corner_radius=50.0,
        shadow={"color": [0, 0, 0, 0.4], "offset": [0, 6], "blur": 16}, padding=pad(14),
        flex=flex("Row", 14, "Center", "Start"),
        constraints={"left": {"type": "Px", "value": 24.0}, "top": {"type": "Px", "value": 24.0}})
    gold = node("gold_panel", {"type": "Panel"}, [
        node("coin", {"type": "Image", "texture_id": ICONS, "tint": [1, 1, 1, 1], "uv_rect": icon_uv(7)},
             width=fixed(34), height=fixed(34)),
        text("gold", "1 240", 28, lin(1.0, 0.85, 0.35), 110, 34, "Left"),
    ], width=fixed(180), height=fixed(56), bg_color=lin(0.06, 0.07, 0.10, 0.72), corner_radius=28.0,
        shadow={"color": [0, 0, 0, 0.4], "offset": [0, 6], "blur": 16}, padding=pad(10),
        flex=flex("Row", 8, "Center", "Center"),
        constraints={"right": {"type": "Px", "value": 24.0}, "top": {"type": "Px", "value": 24.0}})
    abilities = []
    for i, ic in enumerate((0, 5, 2, 4)):
        abilities.append(node(f"ability_{i}", {"type": "Panel"}, [
            node(f"ability_icon_{i}", {"type": "Image", "texture_id": ICONS, "tint": [1, 1, 1, 1],
                                        "uv_rect": icon_uv(ic)}, width=fixed(52), height=fixed(52)),
            node(f"cool_{i}", {"type": "Panel"}, width=fixed(72), height=fixed(0),
                 bg_color=[0.0, 0.0, 0.0, 0.62], corner_radius=14.0,
                 constraints={"left": {"type": "Px", "value": 0.0}, "bottom": {"type": "Px", "value": 0.0}}),
            {**text(f"key_{i}", str(i + 1), 18, INK, 20, 22),
             "style": {**text(f"key_{i}", str(i + 1), 18, INK, 20, 22)["style"],
                       "constraints": {"right": {"type": "Px", "value": 4.0}, "bottom": {"type": "Px", "value": 2.0}}}},
        ], width=fixed(72), height=fixed(72), bg_color=lin(0.13, 0.14, 0.19, 0.9), corner_radius=16.0,
            border_color=lin(0.35, 0.33, 0.30), border_width=2.0,
            shadow={"color": [0, 0, 0, 0.45], "offset": [0, 5], "blur": 10},
            flex=flex("Column", 0, "Center", "Center")))
    bar_row = node("abilities", {"type": "Panel"}, abilities, width=fixed(360), height=fixed(90),
                   flex=flex("Row", 14, "Center", "Center"),
                   constraints={"left": {"type": "Percent", "value": 50.0}, "bottom": {"type": "Px", "value": 26.0}},
                   offset_x=-180.0)
    toast = node("toast", {"type": "Panel"}, [
        text("toast_title", "QUEST UPDATED", 18, EMBER, 380, 22),
        text("toast_text", "The Ember Road", 28, INK, 380, 34),
    ], width=fixed(420), height=fixed(84), bg_color=lin(0.07, 0.08, 0.11, 0.9), corner_radius=18.0,
        shadow=SHADOW, padding=pad(8), flex=flex("Column", 2, "Center", "Center"), opacity=0.0,
        constraints={"left": {"type": "Percent", "value": 50.0}, "top": {"type": "Px", "value": 30.0}},
        offset_x=-210.0)
    menu_btn = button("btn_menu", "Menu", 120, 44, 20)
    menu_btn["style"]["constraints"] = {"right": {"type": "Px", "value": 24.0},
                                        "bottom": {"type": "Px", "value": 24.0}}
    root = node("root", {"type": "Panel"}, [
        node("card", {"type": "Panel"}, [status, gold, bar_row, toast, menu_btn], width=pct(100), height=pct(100)),
    ], width=pct(100), height=pct(100))
    return doc("HUD", root)


def docs():
    return {"ui/mh_menu.gui": menu_doc(), "ui/mh_inventory.gui": inventory_doc(), "ui/mh_hud.gui": hud_doc()}


def items_lua():
    rows = []
    for name, icon, atk, dfn, wgt, desc in ITEMS:
        rows.append(f'    {{ name = "{name}", icon = {icon}, atk = {atk}, def = {dfn}, wgt = {wgt}, desc = "{desc}" }},')
    return "\n".join([
        "-- Generated by tools/gen_scenes.py from tools/menus.py: the inventory's items.",
        "local M = {}",
        f"M.SLOTS = {SLOTS}",
        f"M.N_ICONS = {N_ICONS}",
        "M.ITEMS = {", *rows, "}",
        "return M", ""])
