/**
 * Editing a behaviour tree (`behavior_tree.root`, Rust
 * `ecs::behavior_tree::BtNode`) as the plain JSON the component holds.
 * A node is `{ type, label?, ... }`; a composite has `children`, a decorator
 * one `child`, a leaf neither. A node is found by its **path**: the child
 * indices from the root (`[]` is the root, `[0, 2]` its first child's
 * third). Every operation returns a new tree.
 *
 * The node types and their fields come from `behaviorTreeNodes.json`, which
 * a Rust test checks against the enum.
 */
import catalogue from "./behaviorTreeNodes.json";

export type BtNode = { type: string; label?: string; children?: BtNode[]; child?: BtNode; [k: string]: unknown };
export type Path = number[];

export interface NodeField {
  name: string;
  kind: "string" | "number" | "int" | "bool" | "json" | "enum";
  doc: string;
  options?: string[];
}

export interface NodeKind {
  type: string;
  group: "composite" | "decorator" | "condition" | "action";
  doc: string;
  default: BtNode;
  fields: NodeField[];
}

export const NODE_KINDS: NodeKind[] = (catalogue as { nodes: NodeKind[] }).nodes;

export function kindOf(type: string): NodeKind | undefined {
  return NODE_KINDS.find((k) => k.type === type);
}

/** A fresh node of `type`, as the catalogue starts it. */
export function newNode(type: string): BtNode {
  const kind = kindOf(type);
  return structuredClone(kind ? kind.default : { type });
}

/** How many children a node takes. */
export function arity(node: BtNode): "many" | "one" | "none" {
  const group = kindOf(node.type)?.group;
  if (group === "composite" || Array.isArray(node.children)) return "many";
  if (group === "decorator" || node.child) return "one";
  return "none";
}

/** A node's children, in order. */
export function childrenOf(node: BtNode): BtNode[] {
  if (Array.isArray(node.children)) return node.children;
  return node.child ? [node.child] : [];
}

export function getNode(root: BtNode, path: Path): BtNode | undefined {
  let node: BtNode | undefined = root;
  for (const i of path) node = node ? childrenOf(node)[i] : undefined;
  return node;
}

/** `fn` applied to a copy of the tree's node at `path`; the root is copied
 *  whole so the original is untouched. */
function edit(root: BtNode, path: Path, fn: (node: BtNode) => void): BtNode {
  const out = structuredClone(root);
  const node = getNode(out, path);
  if (node) fn(node);
  return out;
}

/** Replace the node at `path` (its children kept when the new one can hold
 *  them - changing a sequence into a selector keeps its branch). */
export function replaceNode(root: BtNode, path: Path, next: BtNode): BtNode {
  if (path.length === 0) return structuredClone(next);
  const parentPath = path.slice(0, -1);
  const at = path[path.length - 1];
  return edit(root, parentPath, (parent) => {
    if (Array.isArray(parent.children)) parent.children[at] = structuredClone(next);
    else parent.child = structuredClone(next);
  });
}

/** Turn the node at `path` into another type: its label and whatever fields
 *  the two share are kept, and so are its children if the new type can hold
 *  them (all of them for a composite, the first for a decorator). */
export function changeType(root: BtNode, path: Path, type: string): BtNode {
  const old = getNode(root, path);
  if (!old) return root;
  const next = newNode(type);
  if (old.label) next.label = old.label;
  for (const f of kindOf(type)?.fields ?? []) {
    if (f.name in old) next[f.name] = structuredClone(old[f.name]);
  }
  const kids = childrenOf(old);
  const a = arity(next);
  if (a === "many") next.children = structuredClone(kids);
  else if (a === "one" && kids.length > 0) next.child = structuredClone(kids[0]);
  return replaceNode(root, path, next);
}

/** Add `node` as the last child of the node at `path`. A decorator holds
 *  one child, so adding to it replaces that child. Returns the tree and the
 *  new node's path, or null when the node takes no children. */
export function addChild(root: BtNode, path: Path, node: BtNode): { root: BtNode; path: Path } | null {
  const parent = getNode(root, path);
  if (!parent) return null;
  const a = arity(parent);
  if (a === "none") return null;
  if (a === "one") return { root: edit(root, path, (p) => (p.child = structuredClone(node))), path: [...path, 0] };
  const index = childrenOf(parent).length;
  return {
    root: edit(root, path, (p) => {
      p.children = [...(p.children ?? []), structuredClone(node)];
    }),
    path: [...path, index],
  };
}

/** Remove the node at `path`. A decorator's only child is not removed (it
 *  must have one) and neither is the root. Null when nothing was removed. */
export function removeNode(root: BtNode, path: Path): BtNode | null {
  if (path.length === 0) return null;
  const parent = getNode(root, path.slice(0, -1));
  if (!parent || arity(parent) !== "many") return null;
  const at = path[path.length - 1];
  return edit(root, path.slice(0, -1), (p) => {
    p.children = (p.children ?? []).filter((_, i) => i !== at);
  });
}

/** Move the node at `path` up (-1) or down (+1) among its siblings. Returns
 *  the tree and the node's new path, or null at the end of the list. */
export function moveNode(root: BtNode, path: Path, delta: -1 | 1): { root: BtNode; path: Path } | null {
  if (path.length === 0) return null;
  const parentPath = path.slice(0, -1);
  const parent = getNode(root, parentPath);
  const at = path[path.length - 1];
  const to = at + delta;
  if (!parent || !Array.isArray(parent.children) || to < 0 || to >= parent.children.length) return null;
  return {
    root: edit(root, parentPath, (p) => {
      const kids = p.children!;
      [kids[at], kids[to]] = [kids[to], kids[at]];
    }),
    path: [...parentPath, to],
  };
}

/** Wrap the node at `path` in a decorator (`invert`, `repeat`, ...). */
export function wrapNode(root: BtNode, path: Path, decorator: string): BtNode {
  const node = getNode(root, path);
  if (!node) return root;
  const wrapper = newNode(decorator);
  wrapper.child = structuredClone(node);
  delete wrapper.children;
  return replaceNode(root, path, wrapper);
}

/** One row of the outline: the node, its path and its depth. */
export interface Row {
  node: BtNode;
  path: Path;
  depth: number;
}

/** The tree as outline rows, root first, children under their parent. */
export function flatten(root: BtNode): Row[] {
  const rows: Row[] = [];
  const walk = (node: BtNode, path: Path) => {
    rows.push({ node, path, depth: path.length });
    childrenOf(node).forEach((c, i) => walk(c, [...path, i]));
  };
  walk(root, []);
  return rows;
}

/** What the tree's `_running` blackboard readout calls a node - its label,
 *  else a name by type (Rust `BtNode::shown`) - so a running leaf can be
 *  picked out of the outline while playing. */
export function shownName(node: BtNode): string {
  if (node.label) return node.label;
  if (node.type === "wait") return "wait";
  if (node.type === "move_to") return node.away ? "flee" : "move_to";
  if (node.type === "call") return String(node.call ?? "");
  return "node";
}

/** A one-line summary of a node's settings for its outline row. */
export function summary(node: BtNode): string {
  const f = (k: string) => (node[k] === undefined ? "" : typeof node[k] === "string" ? String(node[k]) : JSON.stringify(node[k]));
  switch (node.type) {
    case "check":
      return `${f("key")} ${f("op") || "=="} ${node.op === "set" || node.op === "unset" ? "" : f("value")}`.trim();
    case "chance":
      return `p ${f("p")}`;
    case "near":
      return `${f("target")} within ${f("distance")}`;
    case "wait":
      return `${f("seconds")}s` + (node.jitter ? ` +${f("jitter")}` : "");
    case "move_to":
      return `${node.away ? "away from" : "to"} ${f("target")}`;
    case "set":
      return `${f("key")} ${node.add ? "+=" : "="} ${f("value")}`;
    case "write":
      return `${node.target ? f("target") + "." : ""}${f("component")}.${f("field")} = ${f("value")}`;
    case "event":
      return `on_${f("event")}`;
    case "call":
      return `on_${f("call")}`;
    case "repeat":
      return node.until_failure ? "until failure" : node.times ? `${f("times")}x` : "forever";
    case "cooldown":
    case "timeout":
      return `${f("seconds")}s`;
    case "parallel":
      return node.succeed ? `${f("succeed")} to succeed` : "all to succeed";
    case "sequence":
    case "selector":
      return node.reactive ? "reactive" : "";
    default:
      return "";
  }
}
