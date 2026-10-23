import { invoke } from "@tauri-apps/api/core";

/** The project's `server` block, as the Servers panel needs it. */
export interface ServerConfig {
  package: string;
  bin: string;
  address: string;
  args: string[];
  service: string;
  fleet: string | null;
  fleet_exists: boolean;
  local_agent_port: number;
  admin_url: string;
}

/** How a server answers: health round trip and, with the token, its overview. */
export interface Probe {
  up: boolean;
  ms: number | null;
  overview: any | null;
  error: string | null;
}

export interface LocalStatus {
  running: boolean;
  address: string;
  pid?: number;
  uptime?: number;
  /** Whether the running server was started for this project. */
  mine?: boolean;
  /** Set once when the server stopped by itself: its exit status. */
  exited?: string;
  probe: Probe;
}

export interface FleetService {
  name: string;
  version: string | null;
  previous: string | null;
  port: number | null;
  status: string;
  pid: number | null;
  uptime: number | null;
  restarts: number;
  last_error: string | null;
  probe?: Probe;
}

export interface FleetMachine {
  name: string;
  url: string;
  /** The machine's console (a host's full management page), if the fleet file names one. */
  console?: string | null;
  info?: { platform: string; version: string; services: FleetService[] };
  error?: string;
}

export interface FleetStatus {
  service: string;
  machines: FleetMachine[];
  local_agent: string | null;
}

export const serverConfig = (projectPath: string) =>
  invoke<ServerConfig | null>("server_config", { projectPath });
export const serverStart = (projectPath: string) => invoke<string>("server_start", { projectPath });
export const serverStop = () => invoke<string>("server_stop");
export const serverStatus = (projectPath: string) => invoke<LocalStatus>("server_status", { projectPath });
export const serverNotice = (projectPath: string, text: string) =>
  invoke<void>("server_notice", { projectPath, text });

export const serverOpenDashboard = (projectPath: string, address: string | null = null) =>
  invoke<void>("server_open_dashboard", { projectPath, address });

export const fleetCreate = (projectPath: string) => invoke<string>("fleet_create", { projectPath });
export const fleetStatus = (projectPath: string) => invoke<FleetStatus>("fleet_status", { projectPath });
export const fleetDeploy = (projectPath: string, machine: string | null) =>
  invoke<void>("fleet_deploy", { projectPath, machine });
export const fleetAct = (projectPath: string, action: string, machine: string | null) =>
  invoke<void>("fleet_act", { projectPath, action, machine });
export const fleetLogs = (projectPath: string, machine: string, lines = 200) =>
  invoke<string>("fleet_logs", { projectPath, machine, lines });
export const fleetOpenConsole = (projectPath: string, machine: string) =>
  invoke<void>("fleet_open_console", { projectPath, machine });
export const fleetLocalAgent = (projectPath: string, on: boolean) =>
  invoke<string | null>("fleet_local_agent", { projectPath, on });
