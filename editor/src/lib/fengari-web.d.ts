// Minimal ambient declaration for fengari-web (the package ships no types).
// We only need to satisfy the Lua runtime shim's imports; everything is `any`.
declare module 'fengari-web' {
  export const lua: any;
  export const lauxlib: any;
  export const lualib: any;
  export const interop: any;
  export const L: any;
  export function load(source: string, chunkname?: string): any;
  export function to_luastring(s: string, cache?: boolean): Uint8Array;
  export function to_jsstring(b: Uint8Array): string;
  export function to_uristring(b: Uint8Array): string;
  export function luastring_eq(a: Uint8Array, b: Uint8Array): boolean;
  export function luastring_indexOf(b: Uint8Array, ch: number, start?: number): number;
  export function luastring_of(...bytes: number[]): Uint8Array;
  export const FENGARI_AUTHORS: string;
  export const FENGARI_COPYRIGHT: string;
  export const FENGARI_RELEASE: string;
  export const FENGARI_VERSION: string;
  export const FENGARI_VERSION_MAJOR: string;
  export const FENGARI_VERSION_MINOR: string;
  export const FENGARI_VERSION_NUM: number;
  export const FENGARI_VERSION_RELEASE: string;
}
