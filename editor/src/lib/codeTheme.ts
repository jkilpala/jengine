import { EditorView } from "@codemirror/view";
import { HighlightStyle, syntaxHighlighting } from "@codemirror/language";
import { tags as t } from "@lezer/highlight";
import type { Extension } from "@codemirror/state";

// The script editor's look, in the editor palette's tokens (theme.css). The
// colours are `var(--ed-*)`, so a palette change restyles an open editor
// with no rebuild; only CodeMirror's own light / dark flag, which picks the
// defaults this does not override, follows `dark`. Syntax colours take the
// website's code blocks (synthwave '84) as the model: keywords in the
// accent, strings warm, functions in the second accent.

export function codeTheme(dark: boolean): Extension {
  const view = EditorView.theme(
    {
      "&": {
        color: "var(--ed-text)",
        backgroundColor: "var(--ed-base)",
      },
      ".cm-content": {
        caretColor: "var(--ed-accent)",
        fontFamily: "var(--ed-mono)",
      },
      ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--ed-accent)" },
      "&.cm-focused > .cm-scroller > .cm-selectionLayer .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection":
        { backgroundColor: "var(--ed-accent-low)" },
      ".cm-panels": { backgroundColor: "var(--ed-mantle)", color: "var(--ed-text)" },
      ".cm-panels.cm-panels-top": { borderBottom: "1px solid var(--ed-line)" },
      ".cm-panels.cm-panels-bottom": { borderTop: "1px solid var(--ed-line)" },
      ".cm-searchMatch": {
        backgroundColor: "color-mix(in srgb, var(--ed-warn) 22%, transparent)",
        outline: "1px solid color-mix(in srgb, var(--ed-warn) 55%, transparent)",
      },
      ".cm-searchMatch.cm-searchMatch-selected": {
        backgroundColor: "color-mix(in srgb, var(--ed-accent) 30%, transparent)",
      },
      ".cm-activeLine": { backgroundColor: "color-mix(in srgb, var(--ed-hover) 55%, transparent)" },
      ".cm-selectionMatch": { backgroundColor: "color-mix(in srgb, var(--ed-a2) 14%, transparent)" },
      "&.cm-focused .cm-matchingBracket, &.cm-focused .cm-nonmatchingBracket": {
        backgroundColor: "color-mix(in srgb, var(--ed-a2) 22%, transparent)",
        outline: "1px solid color-mix(in srgb, var(--ed-a2) 45%, transparent)",
      },
      ".cm-gutters": {
        backgroundColor: "var(--ed-mantle)",
        color: "var(--ed-faint)",
        border: "none",
        borderRight: "1px solid var(--ed-line)",
        fontFamily: "var(--ed-mono)",
      },
      ".cm-activeLineGutter": { backgroundColor: "var(--ed-hover)", color: "var(--ed-text)" },
      ".cm-foldPlaceholder": {
        backgroundColor: "var(--ed-raised)",
        border: "1px solid var(--ed-line)",
        color: "var(--ed-muted)",
      },
      ".cm-tooltip": {
        border: "1px solid var(--ed-line-strong)",
        backgroundColor: "var(--ed-raised)",
        color: "var(--ed-text)",
        borderRadius: "6px",
        boxShadow: "0 10px 26px rgba(0, 0, 0, 0.4)",
      },
      ".cm-tooltip .cm-tooltip-arrow:before": { borderTopColor: "transparent", borderBottomColor: "transparent" },
      ".cm-tooltip .cm-tooltip-arrow:after": {
        borderTopColor: "var(--ed-raised)",
        borderBottomColor: "var(--ed-raised)",
      },
      ".cm-tooltip-autocomplete": {
        "& > ul": { fontFamily: "var(--ed-mono)" },
        "& > ul > li[aria-selected]": {
          backgroundColor: "var(--ed-accent-low)",
          color: "var(--ed-ink)",
        },
      },
      ".cm-completionDetail": { color: "var(--ed-faint)" },
      ".cm-completionMatchedText": { color: "var(--ed-accent)", textDecoration: "none" },
    },
    { dark },
  );

  const highlight = HighlightStyle.define([
    { tag: [t.keyword, t.controlKeyword, t.operatorKeyword, t.modifier, t.definitionKeyword, t.moduleKeyword], color: "var(--ed-accent)" },
    { tag: [t.string, t.special(t.string), t.regexp, t.escape], color: "var(--ed-a3)" },
    { tag: [t.number, t.bool, t.null, t.atom, t.constant(t.name)], color: "var(--ed-cat-4)" },
    { tag: [t.function(t.variableName), t.function(t.propertyName), t.macroName], color: "var(--ed-a2)" },
    { tag: [t.definition(t.variableName), t.definition(t.propertyName)], color: "var(--ed-ink)" },
    { tag: [t.propertyName, t.attributeName], color: "var(--ed-accent-high)" },
    { tag: [t.typeName, t.className, t.namespace, t.self], color: "var(--ed-cat-6)" },
    { tag: [t.variableName, t.name], color: "var(--ed-text)" },
    { tag: [t.operator, t.punctuation, t.separator, t.bracket], color: "var(--ed-muted)" },
    { tag: [t.comment, t.lineComment, t.blockComment, t.docComment], color: "var(--ed-faint)", fontStyle: "italic" },
    { tag: [t.meta, t.labelName], color: "var(--ed-muted)" },
    { tag: t.link, color: "var(--ed-a2)", textDecoration: "underline" },
    { tag: t.heading, color: "var(--ed-accent)", fontWeight: "600" },
    { tag: t.emphasis, fontStyle: "italic" },
    { tag: t.strong, fontWeight: "600" },
    { tag: t.strikethrough, textDecoration: "line-through" },
    { tag: t.invalid, color: "var(--ed-err)" },
  ]);

  return [view, syntaxHighlighting(highlight)];
}
