<script lang="ts">
  // A behaviour tree (`behavior_tree.root`) as an outline: one row a node,
  // children indented under their parent, coloured by what the node is
  // (composite, decorator, condition, action). Select a row to edit its
  // fields below, add a child to it, wrap it in a decorator, change its type,
  // move it among its siblings or remove it. While the game plays, the leaves
  // the tree is running (its `_running` blackboard readout) are lit. Every
  // change reports the whole new root (`behaviorTree.ts`), one undo step.
  import {
    NODE_KINDS,
    addChild,
    arity,
    changeType,
    flatten,
    getNode,
    kindOf,
    moveNode,
    newNode,
    removeNode,
    replaceNode,
    shownName,
    summary,
    wrapNode,
    type BtNode,
    type NodeField,
    type Path,
  } from "./behaviorTree";

  let {
    root,
    running = [],
    disabled = false,
    onchange,
  }: {
    root: BtNode | undefined | null;
    /** The labels the tree's readout says are running. */
    running?: string[];
    disabled?: boolean;
    onchange: (root: BtNode) => void;
  } = $props();

  let tree: BtNode = $derived(root && typeof root === "object" && root.type ? root : newNode("sequence"));
  let rows = $derived(flatten(tree));
  let selected = $state<Path>([]);
  let node = $derived(getNode(tree, selected) ?? tree);
  let kind = $derived(kindOf(node.type));
  let jsonErrors = $state<Record<string, string>>({});

  const same = (a: Path, b: Path) => a.length === b.length && a.every((v, i) => v === b[i]);
  const COLOURS: Record<string, string> = {
    composite: "var(--ed-cat-5)",
    decorator: "var(--ed-cat-6)",
    condition: "var(--ed-cat-4)",
    action: "var(--ed-cat-7)",
  };
  const groups = ["composite", "decorator", "condition", "action"] as const;
  const decorators = NODE_KINDS.filter((k) => k.group === "decorator");

  function commit(next: BtNode, select?: Path) {
    if (select) selected = select;
    onchange(next);
  }

  function setField(field: NodeField, value: unknown) {
    const next = structuredClone(node);
    if (value === undefined || value === "") delete next[field.name];
    else next[field.name] = value;
    commit(replaceNode(tree, selected, next));
  }

  function setLabel(label: string) {
    const next = structuredClone(node);
    if (label.trim()) next.label = label.trim();
    else delete next.label;
    commit(replaceNode(tree, selected, next));
  }

  function setJson(field: NodeField, text: string) {
    const t = text.trim();
    if (t === "") {
      jsonErrors = { ...jsonErrors, [field.name]: "" };
      setField(field, null);
      return;
    }
    try {
      const value = JSON.parse(t);
      jsonErrors = { ...jsonErrors, [field.name]: "" };
      setField(field, value);
    } catch {
      // A bare word is meant as a string: `Player`, `@goal`.
      if (/^[@\w:.\-]+$/.test(t)) {
        jsonErrors = { ...jsonErrors, [field.name]: "" };
        setField(field, t);
      } else {
        jsonErrors = { ...jsonErrors, [field.name]: "not JSON" };
      }
    }
  }

  function add(type: string) {
    const r = addChild(tree, selected, newNode(type));
    if (r) commit(r.root, r.path);
  }

  function move(delta: -1 | 1) {
    const r = moveNode(tree, selected, delta);
    if (r) commit(r.root, r.path);
  }

  function remove() {
    const r = removeNode(tree, selected);
    if (r) commit(r, selected.slice(0, -1));
  }

  function onKey(e: KeyboardEvent) {
    const at = rows.findIndex((r) => same(r.path, selected));
    if (e.key === "ArrowDown" && at < rows.length - 1) selected = rows[at + 1].path;
    else if (e.key === "ArrowUp" && at > 0) selected = rows[at - 1].path;
    else if ((e.key === "Delete" || e.key === "Backspace") && !disabled) remove();
    else return;
    e.preventDefault();
  }

  const text = (v: unknown) => (v === undefined || v === null ? "" : typeof v === "string" ? v : JSON.stringify(v));
</script>

<div class="bt">
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="outline" tabindex="0" role="tree" aria-label="Behaviour tree" onkeydown={onKey}>
    {#each rows as row (row.path.join("."))}
      {@const k = kindOf(row.node.type)}
      {@const live = running.includes(shownName(row.node)) && arity(row.node) === "none"}
      <button
        class="row"
        class:selected={same(row.path, selected)}
        class:live
        style:padding-left="{6 + row.depth * 14}px"
        role="treeitem"
        aria-selected={same(row.path, selected)}
        title={k?.doc ?? row.node.type}
        onclick={() => (selected = row.path)}
      >
        <span class="dot" style:background={COLOURS[k?.group ?? "action"]}></span>
        <span class="type">{row.node.type}</span>
        {#if row.node.label}<span class="label">"{row.node.label}"</span>{/if}
        <span class="summary">{summary(row.node)}</span>
      </button>
    {/each}
  </div>

  <div class="tools">
    {#if arity(node) !== "none"}
      <select
        {disabled}
        title={arity(node) === "one" ? "Replace this decorator's child" : "Add a child at the end"}
        onchange={(e) => {
          const el = e.target as HTMLSelectElement;
          if (el.value) add(el.value);
          el.value = "";
        }}
      >
        <option value="">{arity(node) === "one" ? "child…" : "+ child…"}</option>
        {#each groups as g}
          <optgroup label={g}>
            {#each NODE_KINDS.filter((k) => k.group === g) as k}
              <option value={k.type}>{k.type}</option>
            {/each}
          </optgroup>
        {/each}
      </select>
    {/if}
    <select
      {disabled}
      title="Wrap this node in a decorator"
      onchange={(e) => {
        const el = e.target as HTMLSelectElement;
        if (el.value) commit(wrapNode(tree, selected, el.value));
        el.value = "";
      }}
    >
      <option value="">wrap…</option>
      {#each decorators as d}
        <option value={d.type}>{d.type}</option>
      {/each}
    </select>
    <select
      {disabled}
      title="Change this node's type (its label, shared fields and children are kept)"
      value={node.type}
      onchange={(e) => commit(changeType(tree, selected, (e.target as HTMLSelectElement).value))}
    >
      {#each groups as g}
        <optgroup label={g}>
          {#each NODE_KINDS.filter((k) => k.group === g) as k}
            <option value={k.type}>{k.type}</option>
          {/each}
        </optgroup>
      {/each}
    </select>
    <button {disabled} title="Move up" onclick={() => move(-1)}>↑</button>
    <button {disabled} title="Move down" onclick={() => move(1)}>↓</button>
    <button {disabled} title="Remove (a decorator's only child, and the root, cannot be removed)" onclick={remove}>×</button>
  </div>

  <div class="form">
    {#if kind}<div class="doc">{kind.doc}</div>{/if}
    <label class="field">
      <span>label</span>
      <input
        type="text"
        value={node.label ?? ""}
        placeholder="(shown in the running readout)"
        {disabled}
        onchange={(e) => setLabel((e.target as HTMLInputElement).value)}
      />
    </label>
    {#each kind?.fields ?? [] as f (f.name)}
      <label class="field" title={f.doc}>
        <span>{f.name}</span>
        {#if f.kind === "bool"}
          <input type="checkbox" checked={!!node[f.name]} {disabled} onchange={(e) => setField(f, (e.target as HTMLInputElement).checked || undefined)} />
        {:else if f.kind === "number" || f.kind === "int"}
          <input
            type="number"
            step={f.kind === "int" ? 1 : 0.1}
            value={node[f.name] ?? ""}
            {disabled}
            onchange={(e) => {
              // A blank number keeps the old one: `seconds` and the like are
              // required, and a node without one breaks the tree on load.
              const v = parseFloat((e.target as HTMLInputElement).value);
              if (Number.isFinite(v)) setField(f, f.kind === "int" ? Math.round(v) : v);
            }}
          />
        {:else if f.kind === "enum"}
          <select value={node[f.name] ?? f.options?.[0]} {disabled} onchange={(e) => setField(f, (e.target as HTMLSelectElement).value)}>
            {#each f.options ?? [] as o}<option value={o}>{o}</option>{/each}
          </select>
        {:else if f.kind === "json"}
          <input
            type="text"
            class="mono"
            value={text(node[f.name])}
            {disabled}
            onchange={(e) => setJson(f, (e.target as HTMLInputElement).value)}
          />
        {:else}
          <input type="text" value={node[f.name] ?? ""} {disabled} onchange={(e) => setField(f, (e.target as HTMLInputElement).value)} />
        {/if}
      </label>
      {#if jsonErrors[f.name]}<span class="error">{jsonErrors[f.name]}</span>{/if}
    {/each}
  </div>
</div>

<style>
  .bt {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }
  .outline {
    display: flex;
    flex-direction: column;
    max-height: 260px;
    overflow-y: auto;
    border: 1px solid var(--ed-line);
    border-radius: 3px;
    background: var(--ed-base);
    outline: none;
  }
  .outline:focus-visible {
    border-color: var(--ed-accent);
  }
  .row {
    display: flex;
    align-items: center;
    gap: 5px;
    border: none;
    background: none;
    color: var(--ed-text);
    font-size: 11px;
    text-align: left;
    padding: 2px 6px;
    cursor: pointer;
    white-space: nowrap;
    overflow: hidden;
  }
  .row:hover {
    background: var(--ed-raised);
  }
  .row.selected {
    background: var(--ed-line);
  }
  .row.live .type {
    color: var(--ed-ok);
    font-weight: 700;
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    flex-shrink: 0;
  }
  .type {
    font-family: var(--ed-mono);
  }
  .label {
    color: var(--ed-accent-high);
  }
  .summary {
    color: var(--ed-faint);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .tools {
    display: flex;
    gap: 3px;
    flex-wrap: wrap;
  }
  .tools select,
  .tools button,
  .form input,
  .form select {
    background: var(--ed-base);
    border: 1px solid var(--ed-line);
    color: var(--ed-text);
    border-radius: 4px;
    font-size: 11.5px;
    padding: 3px 4px;
  }
  .tools button {
    cursor: pointer;
    padding: 3px 7px;
  }
  .form {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .doc {
    font-size: 10px;
    color: var(--ed-muted);
  }
  .field {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 11px;
  }
  .field span {
    min-width: 64px;
    color: var(--ed-muted);
  }
  .field input[type="text"],
  .field input[type="number"],
  .field select {
    flex: 1;
    min-width: 0;
  }
  .mono {
    font-family: var(--ed-mono);
  }
  .error {
    color: var(--ed-err);
    font-size: 10px;
  }
</style>
