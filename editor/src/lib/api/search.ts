import { invoke } from "@tauri-apps/api/core";

/** One matching line. Mirrors `SearchHit` in `commands/search.rs`. */
export interface SearchHit {
  /** Project-root-relative, forward slashes (`assets/scripts/a.lua`). */
  path: string;
  /** 1-based. */
  line: number;
  /** `[start, end)` character offsets into `text`, for highlighting. */
  matches: [number, number][];
  text: string;
  /** The source line was longer than the backend's display limit. */
  clipped: boolean;
}

/** Mirrors `SearchResults` in `commands/search.rs`. */
export interface SearchResults {
  hits: SearchHit[];
  files_matched: number;
  files_searched: number;
  /** The backend's hit cap was reached and the walk stopped early. */
  truncated: boolean;
}

export interface SearchOptions {
  caseSensitive: boolean;
  wholeWord: boolean;
  isRegex: boolean;
}

/**
 * Search every text file in the project.
 *
 * Rejects rather than resolving on a bad pattern, so the caller can show the
 * regex error next to the input instead of an empty result list that looks
 * like "no matches".
 */
export function searchInFiles(
  projectPath: string,
  query: string,
  opts: SearchOptions,
): Promise<SearchResults> {
  return invoke<SearchResults>("search_in_files", {
    projectPath,
    query,
    caseSensitive: opts.caseSensitive,
    wholeWord: opts.wholeWord,
    isRegex: opts.isRegex,
  });
}

/** One line a replace would change. Mirrors `ReplacePreviewHit`. */
export interface ReplacePreviewHit {
  path: string;
  line: number;
  before: string;
  after: string;
}

/** Mirrors `ReplacePreview`. Produced without writing anything. */
export interface ReplacePreview {
  hits: ReplacePreviewHit[];
  files: string[];
  truncated: boolean;
}

/** Mirrors `ReplaceSummary`. */
export interface ReplaceSummary {
  files_changed: number;
  replacements: number;
  failures: string[];
}

/** What a replace would change. Writes nothing. */
export function previewReplace(
  projectPath: string,
  query: string,
  replacement: string,
  opts: SearchOptions,
): Promise<ReplacePreview> {
  return invoke<ReplacePreview>("preview_replace_in_files", {
    projectPath,
    query,
    replacement,
    caseSensitive: opts.caseSensitive,
    wholeWord: opts.wholeWord,
    isRegex: opts.isRegex,
  });
}

/**
 * Apply a replacement to `paths`.
 *
 * `paths` comes from a preview the user approved, not from a fresh search, so
 * a file created between previewing and confirming is never swept up.
 */
export function applyReplace(
  projectPath: string,
  query: string,
  replacement: string,
  opts: SearchOptions,
  paths: string[],
): Promise<ReplaceSummary> {
  return invoke<ReplaceSummary>("apply_replace_in_files", {
    projectPath,
    query,
    replacement,
    caseSensitive: opts.caseSensitive,
    wholeWord: opts.wholeWord,
    isRegex: opts.isRegex,
    paths,
  });
}
