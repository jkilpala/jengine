/**
 * CodeMirror autocomplete for jEngine scripts.
 *
 * The API comes from `generated/script-api.json`, which is generated from the
 * engine's own lists (`scripting/namespaces.rs`, the API Index in
 * `docs/scripting-api.md`, the packages' commands) by
 * `editor/scripts/script-api/generate.ts` - so a completion names a function
 * some runtime installs, under the name scripts call it by. Lua scripts call
 * capitalised namespaces (`Sprite.set_color(e, ...)`); JavaScript scripts keep
 * the flat globals (`set_sprite_color(e, ...)`), so they are offered those.
 *
 * The callbacks a script defines (`init`, `update`, `on_collision_enter`, ...)
 * are `scriptCallbacks.ts`; the Lua keywords and snippets are below.
 * `scripts/script-api/scriptApi.test.ts` holds all of it to the engine.
 */
import {
  snippetCompletion,
  type Completion,
  type CompletionContext,
  type CompletionResult,
} from "@codemirror/autocomplete";
import scriptApi from "./generated/script-api.json";
import { SCRIPT_CALLBACKS } from "./scriptCallbacks";

interface ApiFunction {
  name: string;
  flat: string;
  args: string;
  returns: string;
  info: string;
}

interface ApiNamespace {
  name: string;
  source: string;
  functions: ApiFunction[];
}

const NAMESPACES: ApiNamespace[] = scriptApi.namespaces;
const GLOBALS: ApiFunction[] = scriptApi.globals;

function detail(fn: ApiFunction): string {
  return fn.returns ? `${fn.args} → ${fn.returns}` : fn.args;
}

// --- Lua: namespaces, globals, callbacks, keywords, snippets ----------------

const byNamespace = new Map<string, Completion[]>(
  NAMESPACES.map((ns) => [
    ns.name,
    ns.functions.map((fn) => ({
      label: `${ns.name}.${fn.name}`,
      type: "function",
      detail: detail(fn),
      info: fn.info,
      boost: 1,
    })),
  ]),
);

const namespaceCompletions: Completion[] = NAMESPACES.map((ns) => ({
  label: ns.name,
  type: "namespace",
  detail: ns.source === "core" ? "namespace" : `namespace · ${ns.source}`,
  info:
    ns.source === "core"
      ? `jEngine ${ns.name} functions`
      : `Commands of the ${ns.source} package. It is nil unless the project lists the package: test with \`if ${ns.name} == nil then return end\`.`,
  boost: 2,
}));

const globalCompletions: Completion[] = GLOBALS.map((fn) => ({
  label: fn.name,
  type: "function",
  detail: detail(fn),
  info: fn.info,
}));

/** A callback offered as the function it is: `function update(entity, dt) ... end`. */
const callbackCompletions: Completion[] = SCRIPT_CALLBACKS.map((cb) =>
  snippetCompletion(`function ${cb.name}${cb.args}\n\t\${}\nend`, {
    label: cb.name,
    type: "method",
    detail: `${cb.args} · callback`,
    info: cb.info,
  }),
);

/** Lua 5.1-5.3's reserved words: the subset both Luau and Fengari read. */
export const LUA_KEYWORDS = [
  "and", "break", "do", "else", "elseif", "end", "false", "for", "function",
  "if", "in", "local", "nil", "not", "or", "repeat", "return", "then", "true",
  "until", "while",
];

const keywordCompletions: Completion[] = LUA_KEYWORDS.map((k) => ({
  label: k,
  type: "keyword",
  boost: -1,
}));

const snippetCompletions: Completion[] = [
  snippetCompletion("local function ${name}(${})\n\t${}\nend", {
    label: "local function",
    type: "keyword",
    detail: "snippet",
  }),
  snippetCompletion("if ${condition} then\n\t${}\nend", { label: "if", type: "keyword", detail: "snippet" }),
  snippetCompletion("for ${i} = ${start}, ${stop} do\n\t${}\nend", {
    label: "for",
    type: "keyword",
    detail: "numeric for",
  }),
  snippetCompletion("for ${k}, ${v} in pairs(${t}) do\n\t${}\nend", {
    label: "for pairs",
    type: "keyword",
    detail: "snippet",
  }),
  snippetCompletion("for ${i}, ${v} in ipairs(${t}) do\n\t${}\nend", {
    label: "for ipairs",
    type: "keyword",
    detail: "snippet",
  }),
  snippetCompletion("while ${condition} do\n\t${}\nend", { label: "while", type: "keyword", detail: "snippet" }),
  snippetCompletion('script = { events = { "${on_spawn}" } }', {
    label: "script events",
    type: "keyword",
    detail: "opt in to engine events",
    info: "The header that subscribes a script to on_spawn, on_despawn, on_collision_enter, on_collision_exit and on_trigger.",
  }),
];

const luaBareWord: Completion[] = [
  ...namespaceCompletions,
  ...globalCompletions,
  ...callbackCompletions,
  ...snippetCompletions,
  ...keywordCompletions,
  ...[...byNamespace.values()].flat(),
];

/** Every API name the Lua source offers - what the tests hold to the engine. */
export const LUA_API_LABELS: string[] = [
  ...NAMESPACES.map((ns) => ns.name),
  ...GLOBALS.map((fn) => fn.name),
  ...[...byNamespace.values()].flat().map((c) => c.label),
];

export function jengineCompletionSource(ctx: CompletionContext): CompletionResult | null {
  // `Sprite.` or `Sprite.set_c`: that namespace's functions.
  const dotted = ctx.matchBefore(/[A-Za-z_]\w*\.\w*/);
  if (dotted) {
    const ns = dotted.text.slice(0, dotted.text.indexOf("."));
    const options = byNamespace.get(ns);
    if (options) return { from: dotted.from, options, validFor: /^[\w.]*$/ };
    // A field of some other table (`self.x`, `math.fl`): not ours to guess.
    return null;
  }
  const word = ctx.matchBefore(/\w+/);
  if (word) return { from: word.from, options: luaBareWord, validFor: /^[\w.]*$/ };
  if (ctx.explicit) return { from: ctx.pos, options: luaBareWord };
  return null;
}

// --- JavaScript: the flat globals --------------------------------------------

const jsCompletions: Completion[] = [
  ...NAMESPACES.flatMap((ns) =>
    ns.functions.map((fn) => ({
      label: fn.flat,
      type: "function",
      detail: detail(fn),
      info: ns.source === "core" ? fn.info : `${fn.info} (${ns.source})`,
    })),
  ),
  ...globalCompletions,
  ...SCRIPT_CALLBACKS.map((cb) => ({
    label: cb.name,
    type: "method",
    detail: `${cb.args} · callback`,
    info: cb.info,
  })),
];

/** Every API name the JavaScript source offers. */
export const JS_API_LABELS: string[] = [
  ...NAMESPACES.flatMap((ns) => ns.functions.map((fn) => fn.flat)),
  ...GLOBALS.map((fn) => fn.name),
];

export function jengineJsCompletionSource(ctx: CompletionContext): CompletionResult | null {
  if (ctx.matchBefore(/\.\w*/)) return null;
  const word = ctx.matchBefore(/\w+/);
  if (word) return { from: word.from, options: jsCompletions, validFor: /^\w*$/ };
  if (ctx.explicit) return { from: ctx.pos, options: jsCompletions };
  return null;
}
