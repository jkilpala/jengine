import { writable, get } from 'svelte/store';

/** Built-in panel identifiers. */
export type BuiltinPanelId = 'scene' | 'assets' | 'inspector' | 'console' | 'history' | 'build' | 'search' | 'servers' | 'timeline' | 'localisation' | 'mixer' | 'debugger' | 'profiler';

/** Panel identifier — built-in panels or dynamic package panels (prefixed `pkg:`). */
export type PanelId = BuiltinPanelId | `pkg:${string}`;

export type ZoneId = 'left' | 'right' | 'bottom';

export interface PanelGroup {
  panels: PanelId[];
  active: PanelId;
}

export interface ZoneLayout {
  groups: PanelGroup[];
}

export interface EditorLayout {
  zones: Record<ZoneId, ZoneLayout>;
  leftWidth: number;
  rightWidth: number;
  bottomHeight: number;
  /** Panels switched off in View > Panels. A panel in no zone and not here is
   *  one this layout predates, and `upgradeLayout` docks it. */
  hidden?: PanelId[];
  /** 2 since the bottom dock started with fewer tabs (Oct 2026). */
  version?: number;
}

/** Info about a registered package panel. */
export interface PackagePanelInfo {
  /** Panel tab label. */
  name: string;
  /** Emoji or icon. */
  icon: string;
  /** Default dock zone. */
  defaultZone: ZoneId;
  /** URL to load in the iframe (resolved at registration time). */
  url: string;
  /** Source package name. */
  packageName: string;
}

/** Display names for built-in panels. */
const BUILTIN_PANEL_NAMES: Record<BuiltinPanelId, string> = {
  scene: 'Scene',
  assets: 'Assets',
  inspector: 'Inspector',
  console: 'Console',
  history: 'History',
  build: 'Build',
  search: 'Search',
  servers: 'Servers',
  timeline: 'Timeline',
  localisation: 'Localisation',
  mixer: 'Mixer',
  debugger: 'Debugger',
  profiler: 'Profiler',
};

/** Registered package panels, keyed by PanelId (`pkg:<name>`). */
export const packagePanels = writable<Map<string, PackagePanelInfo>>(new Map());

/** Get display name for any panel. */
export function getPanelName(panelId: PanelId): string {
  if (!panelId.startsWith('pkg:')) {
    return BUILTIN_PANEL_NAMES[panelId as BuiltinPanelId] ?? panelId;
  }
  const info = get(packagePanels).get(panelId);
  return info ? `${info.icon} ${info.name}` : panelId;
}

/** Kept for backward compat — DockZone uses getPanelName() instead. */
export const PANEL_NAMES: Record<string, string> = new Proxy(BUILTIN_PANEL_NAMES as Record<string, string>, {
  get(target, prop: string) {
    if (prop in target) return target[prop];
    return getPanelName(prop as PanelId);
  },
});

const STORAGE_KEY = 'jengine-editor-layout';
/**
 * Panels a saved layout must contain to be usable. `build` and `search` are
 * deliberately absent: they shipped after layouts were already saved, and
 * demanding them would throw away everyone's arrangement. `migrateLayout`
 * adds them instead.
 */
const BUILTIN_PANELS: BuiltinPanelId[] = ['scene', 'assets', 'inspector', 'console', 'history'];

/** Every built-in panel, in the order View > Panels lists them. */
export const ALL_BUILTIN_PANELS: BuiltinPanelId[] = [
  'scene', 'assets', 'inspector', 'console', 'build', 'profiler', 'timeline',
  'history', 'search', 'servers', 'localisation', 'mixer', 'debugger',
];

/** Off until asked for: View > Panels, the command palette, or the thing
 *  that needs one (a search, a build, a breakpoint) brings it back. */
const HIDDEN_BY_DEFAULT: BuiltinPanelId[] = ['history', 'search', 'servers', 'localisation', 'mixer', 'debugger'];

const LAYOUT_VERSION = 2;

const DEFAULT_LAYOUT: EditorLayout = {
  zones: {
    left: { groups: [{ panels: ['scene', 'assets'], active: 'scene' }] },
    right: { groups: [{ panels: ['inspector'], active: 'inspector' }] },
    bottom: { groups: [{ panels: ['console', 'build', 'profiler', 'timeline'], active: 'console' }] },
  },
  leftWidth: 250,
  rightWidth: 300,
  bottomHeight: 150,
  hidden: [...HIDDEN_BY_DEFAULT],
  version: LAYOUT_VERSION,
};

/** Where a panel docks when it is shown again. */
function homeZone(panel: PanelId): ZoneId {
  if (panel === 'scene' || panel === 'assets') return 'left';
  if (panel === 'inspector') return 'right';
  if (panel.startsWith('pkg:')) {
    const zone = get(packagePanels).get(panel)?.defaultZone;
    if (zone === 'left' || zone === 'right' || zone === 'bottom') return zone;
  }
  return 'bottom';
}

function docked(l: EditorLayout): Set<string> {
  const found = new Set<string>();
  for (const zone of Object.values(l.zones)) {
    for (const group of zone.groups) {
      for (const p of group.panels) found.add(p);
    }
  }
  return found;
}

/** Is a panel docked somewhere (rather than switched off)? */
export function isPanelShown(l: EditorLayout, panel: PanelId): boolean {
  return docked(l).has(panel);
}

/**
 * Bring a saved layout up to date. Panels added since it was saved are docked
 * beside the console, unless the user switched them off. A layout from before
 * version 2 gets the new, shorter bottom dock: the panels now off by default
 * are switched off - but only where they still sit in the console's group, so
 * a panel someone moved elsewhere stays where they put it.
 */
export function upgradeLayout(l: EditorLayout): EditorLayout {
  const hidden = new Set<string>(l.hidden ?? []);
  if ((l.version ?? 1) < LAYOUT_VERSION) {
    const group = l.zones.bottom?.groups?.find(g => g.panels.includes('console'));
    if (group) {
      for (const panel of HIDDEN_BY_DEFAULT) {
        const i = group.panels.indexOf(panel);
        if (i < 0) continue;
        group.panels.splice(i, 1);
        if (group.active === panel) group.active = group.panels[0];
        hidden.add(panel);
      }
    }
    l.version = LAYOUT_VERSION;
  }
  const present = docked(l);
  for (const panel of ALL_BUILTIN_PANELS) {
    if (present.has(panel) || hidden.has(panel)) continue;
    const bottom = l.zones.bottom;
    const group = bottom?.groups?.find(g => g.panels.includes('console')) ?? bottom?.groups?.[0];
    if (group) {
      group.panels.push(panel);
    } else if (bottom) {
      bottom.groups.push({ panels: [panel], active: panel });
    }
  }
  l.hidden = [...hidden] as PanelId[];
  return l;
}

/** The layout with a panel switched off. */
export function layoutWithPanelHidden(l: EditorLayout, panel: PanelId): EditorLayout {
  const nl = structuredClone(l);
  removePanelFromAll(nl, panel);
  nl.hidden = [...new Set([...(nl.hidden ?? []), panel])];
  return nl;
}

/** The layout with a panel docked again (in its home zone) and in front. */
export function layoutWithPanelShown(l: EditorLayout, panel: PanelId, zone: ZoneId = homeZone(panel)): EditorLayout {
  const nl = structuredClone(l);
  nl.hidden = (nl.hidden ?? []).filter(p => p !== panel);
  for (const z of Object.values(nl.zones)) {
    for (const group of z.groups) {
      if (group.panels.includes(panel)) {
        group.active = panel;
        return nl;
      }
    }
  }
  const target = nl.zones[zone];
  const group = zone === 'bottom'
    ? target.groups.find(g => g.panels.includes('console')) ?? target.groups[0]
    : target.groups[0];
  if (group) {
    group.panels.push(panel);
    group.active = panel;
  } else {
    target.groups.push({ panels: [panel], active: panel });
  }
  return nl;
}

function loadLayout(): EditorLayout {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) {
      const l = JSON.parse(raw) as EditorLayout;
      const found = docked(l);
      for (const p of l.hidden ?? []) found.add(p);
      // Every built-in panel must be docked or switched off; package panels
      // may be missing (their package may be gone).
      if (BUILTIN_PANELS.every(p => found.has(p))) return upgradeLayout(l);
    }
  } catch { /* ignore corrupt data */ }
  return structuredClone(DEFAULT_LAYOUT);
}

export const layout = writable<EditorLayout>(loadLayout());
export const isPanelDragging = writable(false);

layout.subscribe(l => {
  try { localStorage.setItem(STORAGE_KEY, JSON.stringify(l)); } catch { /* ignore */ }
});

/** Below this the bottom zone is too short to read anything in. */
const MIN_REVEAL_HEIGHT = 150;

/**
 * Bring a panel to the front and make sure it can actually be seen.
 *
 * Used when something other than a click needs to show a panel — a build
 * starting, say. The panel may have been closed out of every zone, and the
 * bottom zone may be dragged down to a sliver, so this re-docks and
 * re-opens as needed rather than silently doing nothing.
 */
export function focusPanel(panel: PanelId) {
  layout.update(l => {
    const nl = structuredClone(l);
    nl.hidden = (nl.hidden ?? []).filter(p => p !== panel);
    for (const [zoneId, zone] of Object.entries(nl.zones)) {
      for (const group of zone.groups) {
        if (group.panels.includes(panel)) {
          group.active = panel;
          if (zoneId === 'bottom' && nl.bottomHeight < MIN_REVEAL_HEIGHT) {
            nl.bottomHeight = MIN_REVEAL_HEIGHT;
          }
          return nl;
        }
      }
    }
    // Not docked anywhere — the user closed it. Put it back in the bottom
    // zone, which is where it belongs and where it started.
    const bottom = nl.zones.bottom;
    if (bottom.groups.length > 0) {
      const group = bottom.groups[0];
      group.panels.push(panel);
      group.active = panel;
    } else {
      bottom.groups.push({ panels: [panel], active: panel });
    }
    if (nl.bottomHeight < MIN_REVEAL_HEIGHT) nl.bottomHeight = MIN_REVEAL_HEIGHT;
    return nl;
  });
}

/** View > Panels: switch a panel off, or dock it again and bring it to the front. */
export function togglePanel(panel: PanelId) {
  layout.update(l => (isPanelShown(l, panel) ? layoutWithPanelHidden(l, panel) : layoutWithPanelShown(l, panel)));
}

export function setActivePanel(zone: ZoneId, groupIdx: number, panel: PanelId) {
  layout.update(l => {
    const nl = structuredClone(l);
    const g = nl.zones[zone]?.groups[groupIdx];
    if (g?.panels.includes(panel)) g.active = panel;
    return nl;
  });
}

function removePanelFromAll(l: EditorLayout, panelId: PanelId) {
  for (const zone of Object.values(l.zones)) {
    for (const group of zone.groups) {
      const idx = group.panels.indexOf(panelId);
      if (idx >= 0) {
        group.panels.splice(idx, 1);
        if (group.active === panelId) group.active = group.panels[0];
      }
    }
    zone.groups = zone.groups.filter(g => g.panels.length > 0);
  }
}

export function movePanel(
  panelId: PanelId,
  toZone: ZoneId,
  toGroupIdx: number,
  position: 'tab' | 'before' | 'after'
) {
  layout.update(l => {
    const nl = structuredClone(l);
    removePanelFromAll(nl, panelId);
    const tz = nl.zones[toZone];
    if (position === 'tab') {
      if (tz.groups[toGroupIdx]) {
        tz.groups[toGroupIdx].panels.push(panelId);
        tz.groups[toGroupIdx].active = panelId;
      } else {
        tz.groups.push({ panels: [panelId], active: panelId });
      }
    } else if (position === 'before') {
      const idx = Math.min(toGroupIdx, tz.groups.length);
      tz.groups.splice(idx, 0, { panels: [panelId], active: panelId });
    } else {
      const idx = Math.min(toGroupIdx + 1, tz.groups.length);
      tz.groups.splice(idx, 0, { panels: [panelId], active: panelId });
    }
    return nl;
  });
}

export function setZoneSize(key: 'leftWidth' | 'rightWidth' | 'bottomHeight', value: number) {
  layout.update(l => ({ ...l, [key]: value }));
}

export function resetLayout() {
  layout.set(structuredClone(DEFAULT_LAYOUT));
}

// ──────────────────────────── Package panel management ────────────────────────────

/** Register a package panel and add it to its default dock zone. */
export function registerPackagePanel(panelId: PanelId, info: PackagePanelInfo) {
  packagePanels.update(m => {
    const next = new Map(m);
    next.set(panelId, info);
    return next;
  });

  // Add to layout if not already present
  const l = get(layout);
  const allPanels = new Set<string>();
  for (const zone of Object.values(l.zones)) {
    for (const group of zone.groups) {
      for (const p of group.panels) allPanels.add(p);
    }
  }
  if (!allPanels.has(panelId) && !(l.hidden ?? []).includes(panelId)) {
    const targetZone = (['left', 'right', 'bottom'].includes(info.defaultZone)
      ? info.defaultZone
      : 'right') as ZoneId;
    // Add as a new tab in the first group of the target zone, or create a new group
    layout.update(l => {
      const nl = structuredClone(l);
      const tz = nl.zones[targetZone];
      if (tz.groups.length > 0) {
        tz.groups[0].panels.push(panelId);
      } else {
        tz.groups.push({ panels: [panelId], active: panelId });
      }
      return nl;
    });
  }
}

/** Unregister a package panel and remove it from layout. */
export function unregisterPackagePanel(panelId: PanelId) {
  packagePanels.update(m => {
    const next = new Map(m);
    next.delete(panelId);
    return next;
  });

  layout.update(l => {
    const nl = structuredClone(l);
    removePanelFromAll(nl, panelId);
    return nl;
  });
}
