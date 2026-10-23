import { describe, expect, it } from "vitest";
import { get } from "svelte/store";
import {
  appendServerLine, classifyServerLine, clearServerLog, fleetLog, formatUptime, latencyGrade, localLog, roomRows,
  serviceAddress,
} from "./serverStore";

describe("server panel helpers", () => {
  it("classifies a server's log lines", () => {
    expect(classifyServerLine("[2026-10-06T09:06:57Z INFO  jengine_server::server] [server] started")).toBe("plain");
    expect(classifyServerLine("[2026-10-06T09:07:07Z WARN  jengine_server::server] [warn] the disk has 1% left")).toBe("warning");
    expect(classifyServerLine("error: could not compile `my-game-server`")).toBe("error");
    expect(classifyServerLine("== started my-game-server (pid 4) on 127.0.0.1:9001")).toBe("note");
  });

  it("keeps the two logs apart", () => {
    clearServerLog("local");
    clearServerLog("fleet");
    appendServerLine("local", "a");
    appendServerLine("fleet", "b");
    expect(get(localLog).map((l) => l.text)).toEqual(["a"]);
    expect(get(fleetLog).map((l) => l.text)).toEqual(["b"]);
  });

  it("formats uptimes and grades round trips", () => {
    expect(formatUptime(42.7)).toBe("42s");
    expect(formatUptime(93.4)).toBe("1m 33s");
    expect(formatUptime(7322)).toBe("2h 2m");
    expect(formatUptime(null)).toBe("-");
    expect(latencyGrade(3)).toBe("good");
    expect(latencyGrade(120)).toBe("fair");
    expect(latencyGrade(400)).toBe("poor");
    expect(latencyGrade(undefined)).toBe("down");
  });

  it("reads the rooms of an admin overview, levels in number order", () => {
    const rows = roomRows({
      rooms: [
        { kind: "level", id: "10", players: 0, tick_ms_avg: 0.2, cpu: 0.2, uptime: 5 },
        { kind: "level", id: "2", players: 3, tick_ms_avg: 0.5, cpu: 0.5, uptime: 9 },
      ],
    });
    expect(rows.map((r) => r.id)).toEqual(["2", "10"]);
    expect(rows[0]).toMatchObject({ kind: "level", players: 3, tickMs: 0.5, cpu: 0.5 });
    expect(roomRows(null)).toEqual([]);
  });

  it("finds a deployed service's dashboard from its agent's URL", () => {
    expect(serviceAddress("http://127.0.0.1:9900", 9101)).toBe("127.0.0.1:9101");
    expect(serviceAddress("https://eu-1.example.com:9900/", 9001)).toBe("eu-1.example.com:9001");
  });
});
