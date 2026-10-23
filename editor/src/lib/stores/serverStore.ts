import { writable } from "svelte/store";

/**
 * The Servers panel's state that outlives the panel: the two logs (the
 * server running here, and builds / deploys), filled from the
 * `server-output` event App.svelte listens to - so a deploy started and
 * then hidden is still in the log when the panel comes back.
 */
export type ServerLogSource = "local" | "fleet";

export interface ServerLine {
  text: string;
  timestamp: number;
  kind: "plain" | "error" | "warning" | "note";
}

const MAX_LINES = 3000;

export const localLog = writable<ServerLine[]>([]);
export const fleetLog = writable<ServerLine[]>([]);

/** Colour only. A server logs through env_logger: `[time LEVEL target] text`. */
export function classifyServerLine(text: string): ServerLine["kind"] {
  if (text.startsWith("== ")) return "note";
  if (/\b(ERROR|error(\[|:))|failed|panicked/.test(text)) return "error";
  if (/\bWARN\b|^warning(\[|:)/.test(text)) return "warning";
  return "plain";
}

export function appendServerLine(source: ServerLogSource, text: string) {
  const store = source === "local" ? localLog : fleetLog;
  store.update((lines) => {
    lines.push({ text, timestamp: Date.now(), kind: classifyServerLine(text) });
    if (lines.length > MAX_LINES) lines.splice(0, lines.length - MAX_LINES);
    return lines;
  });
}

export function clearServerLog(source: ServerLogSource) {
  (source === "local" ? localLog : fleetLog).set([]);
}

/** `93.4` -> `1m 33s`; `7322` -> `2h 2m`. */
export function formatUptime(seconds: number | null | undefined): string {
  if (seconds == null || !Number.isFinite(seconds)) return "-";
  const s = Math.floor(seconds);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
  return `${Math.floor(s / 86400)}d ${Math.floor((s % 86400) / 3600)}h`;
}

/**
 * A connection's quality from its health-check round trip: what a player
 * on the same path would feel. Local servers answer in well under 5 ms; a
 * server across the world in 150+.
 */
export function latencyGrade(ms: number | null | undefined): "good" | "fair" | "poor" | "down" {
  if (ms == null || !Number.isFinite(ms)) return "down";
  if (ms < 60) return "good";
  if (ms < 180) return "fair";
  return "poor";
}

export interface RoomRow {
  kind: string;
  id: string;
  players: number;
  /** Average tick in ms, and the room's share of one core (100 = cannot keep up). */
  tickMs: number | null;
  cpu: number | null;
  uptime: number | null;
}

const num = (v: unknown): number | null => (typeof v === "number" && Number.isFinite(v) ? v : null);

/** A server's rooms, as its admin overview lists them, for a table. */
export function roomRows(overview: any): RoomRow[] {
  const rooms: any[] = Array.isArray(overview?.rooms) ? overview.rooms : [];
  return rooms
    .map((r) => ({
      kind: String(r.kind ?? ""),
      id: String(r.id ?? ""),
      players: num(r.players) ?? 0,
      tickMs: num(r.tick_ms_avg),
      cpu: num(r.cpu),
      uptime: num(r.uptime),
    }))
    .sort((a, b) => a.kind.localeCompare(b.kind) || a.id.localeCompare(b.id, undefined, { numeric: true }));
}

/** A deployed service's `host:port`: its machine's host (from the agent's
 * URL) and the service's own port - where its dashboard is. */
export function serviceAddress(agentUrl: string, port: number): string {
  const host = agentUrl.replace(/^[a-z]+:\/\//i, "").replace(/[:/].*$/, "");
  return `${host}:${port}`;
}
