/**
 * Fuzzy path matching for the Ctrl+P palette.
 *
 * Deliberately in the renderer rather than behind a Tauri command: the palette
 * re-filters on every keystroke, and a round-trip per character is latency the
 * one piece of UI that must feel instant cannot afford. The file list is
 * already in memory by then.
 */

export interface FuzzyResult {
  path: string;
  /** Higher is better. Only meaningful relative to other results. */
  score: number;
  /** Indices into `path` that matched, for highlighting. */
  hit: number[];
}

/**
 * Match `query` against `path`, treating whitespace as separating independent
 * terms that must *all* appear, in any order.
 *
 * `11 scene` finds `scenes/11-text.scene` because both terms are present -
 * matching the whole string as one subsequence cannot, since there is no
 * literal space in the path. Order-independence is what makes this worth
 * having: people type the distinctive part first and remember the folder
 * second.
 *
 * Scores sum across terms and highlights are the union, so a path matching
 * every term in a tight run still outranks one matching them scattered.
 */
export function fuzzyMatch(query: string, path: string): FuzzyResult | null {
  const terms = query.split(/\s+/).filter((t) => t.length > 0);
  if (terms.length === 0) return { path, score: 0, hit: [] };
  if (terms.length === 1) return matchTerm(terms[0], path);

  let score = 0;
  const hit = new Set<number>();
  for (const term of terms) {
    const m = matchTerm(term, path);
    // Every term has to land, or this path is not what was asked for.
    if (!m) return null;
    score += m.score;
    for (const i of m.hit) hit.add(i);
  }
  return { path, score, hit: [...hit].sort((a, b) => a - b) };
}

/** Characters after which a match counts as starting a new "word". */
const BOUNDARY = new Set(["/", "_", "-", ".", " "]);

const SCORE_BASE = 1;
const SCORE_CONSECUTIVE = 8;
const SCORE_BOUNDARY = 6;
/** Per character of gap skipped, so tight matches beat scattered ones. */
const PENALTY_GAP = 0.5;
/** A match inside the filename beats the same match in a directory name. */
const BONUS_BASENAME = 24;
/** The query appearing verbatim, not merely as a subsequence. */
const BONUS_SUBSTRING = 30;

/**
 * Greedy left-to-right subsequence match with positional bonuses.
 *
 * Returns `null` when `query` is not a subsequence of `path` at all.
 *
 * Greedy matching alone picks badly on paths that repeat the query's letters:
 * `pl` against `assets/scripts/lua/player.lua` finds the `p` in `scripts` and
 * the `l` in `lua`, scoring a scattered match and highlighting the wrong
 * characters, while the obvious `pl` in `player` is never considered. So the
 * basename is scored as its own candidate and the better of the two wins —
 * which is the case that matters, because people type filenames.
 */
function matchTerm(term: string, path: string): FuzzyResult | null {
  const q = term.toLowerCase();
  if (!q) return { path, score: 0, hit: [] };

  const full = scan(q, path, 0);

  // Re-score against the filename alone, offsetting the indices back into the
  // full path so highlighting still lines up.
  const slash = path.lastIndexOf("/");
  let best = full;
  if (slash >= 0) {
    const base = scan(q, path.slice(slash + 1), slash + 1);
    if (base && (!best || base.score + BONUS_BASENAME > best.score)) {
      best = { ...base, score: base.score + BONUS_BASENAME };
    }
  }
  if (!best) return null;

  // A verbatim run is what someone typing a name almost always means.
  const lowerPath = path.toLowerCase();
  const at = lowerPath.indexOf(q);
  if (at >= 0) {
    const contiguous: number[] = [];
    for (let i = 0; i < q.length; i++) contiguous.push(at + i);
    const inBase = at > slash;
    const substringScore =
      best.score + BONUS_SUBSTRING + (inBase ? BONUS_BASENAME : 0);
    // Prefer the contiguous highlight too — it is the one a reader expects.
    best = { path, score: substringScore, hit: contiguous };
  }

  return { path, score: best.score, hit: best.hit };
}

/**
 * Match `q` against `text`, reporting indices shifted by `offset`.
 *
 * `offset` exists so the basename can be scored in isolation while still
 * producing highlight indices valid for the whole path.
 */
function scan(
  q: string,
  text: string,
  offset: number,
): { path: string; score: number; hit: number[] } | null {
  const lower = text.toLowerCase();
  const hit: number[] = [];
  let score = 0;
  let qi = 0;
  let prevMatch = -2;

  for (let i = 0; i < lower.length && qi < q.length; i++) {
    if (lower[i] !== q[qi]) continue;

    score += SCORE_BASE;
    if (i === prevMatch + 1) {
      score += SCORE_CONSECUTIVE;
    } else if (prevMatch >= 0) {
      score -= (i - prevMatch - 1) * PENALTY_GAP;
    }
    if (i === 0 || BOUNDARY.has(text[i - 1])) score += SCORE_BOUNDARY;

    hit.push(i + offset);
    prevMatch = i;
    qi++;
  }

  // Every query character has to land somewhere, or this is not a match.
  return qi === q.length ? { path: text, score, hit } : null;
}
