/**
 * The script editor's debugger marks (CodeMirror): a gutter of breakpoints -
 * click a line number's margin, or F9, to set or clear one - and the line the
 * debugger is stopped on, highlighted. The breakpoints themselves live in
 * the debugger (`luaDebugger.ts`); the editor is told them with
 * `setBreakpoints` and the paused line with `setExecutionLine`.
 */
import { RangeSet, StateEffect, StateField, type Extension } from "@codemirror/state";
import { Decoration, EditorView, GutterMarker, gutter, type DecorationSet } from "@codemirror/view";

class BreakpointMarker extends GutterMarker {
  toDOM() {
    const el = document.createElement("span");
    el.className = "cm-breakpoint-dot";
    el.textContent = "●";
    return el;
  }
}
const marker = new BreakpointMarker();

/** Replace the editor's breakpoint lines (1-based). */
export const setBreakpoints = StateEffect.define<number[]>();
/** Set the line the debugger is stopped on, or null. */
export const setExecutionLine = StateEffect.define<number | null>();

const breakpointField = StateField.define<RangeSet<GutterMarker>>({
  create: () => RangeSet.empty,
  update(set, tr) {
    let out = set.map(tr.changes);
    for (const e of tr.effects) {
      if (e.is(setBreakpoints)) {
        const doc = tr.state.doc;
        const ranges = e.value
          .filter((l) => l >= 1 && l <= doc.lines)
          .sort((a, b) => a - b)
          .map((l) => marker.range(doc.line(l).from));
        out = RangeSet.of(ranges);
      }
    }
    return out;
  },
});

const executionField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(deco, tr) {
    let out = deco.map(tr.changes);
    for (const e of tr.effects) {
      if (e.is(setExecutionLine)) {
        const doc = tr.state.doc;
        out =
          e.value !== null && e.value >= 1 && e.value <= doc.lines
            ? Decoration.set([Decoration.line({ class: "cm-exec-line" }).range(doc.line(e.value).from)])
            : Decoration.none;
      }
    }
    return out;
  },
  provide: (f) => EditorView.decorations.from(f),
});

/** The gutter and the highlight; `onToggle` is told the line clicked. */
export function debugGutter(onToggle: (line: number) => void): Extension {
  return [
    breakpointField,
    executionField,
    gutter({
      class: "cm-breakpoint-gutter",
      markers: (v) => v.state.field(breakpointField),
      initialSpacer: () => marker,
      domEventHandlers: {
        mousedown(view, block) {
          onToggle(view.state.doc.lineAt(block.from).number);
          return true;
        },
      },
    }),
    EditorView.baseTheme({
      ".cm-breakpoint-gutter": { width: "14px", cursor: "pointer" },
      ".cm-breakpoint-dot": { color: "var(--ed-err)", fontSize: "11px", paddingLeft: "2px" },
      ".cm-exec-line": { backgroundColor: "color-mix(in srgb, var(--ed-warn) 18%, transparent)", outline: "1px solid color-mix(in srgb, var(--ed-warn) 50%, transparent)" },
    }),
  ];
}
