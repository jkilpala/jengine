/**
 * repo-links.ts
 *
 * The site's generated pages come from markdown written for the repository
 * (docs/*.md, packages' READMEs), whose relative links - `[x](parity-gaps.md)`,
 * `[y](../crates/jengine-server/README.md)` - point at files that only exist
 * in the repository. On the site they are dead links, so they become their
 * text. Absolute URLs, site paths (`/...`), anchors and images are kept.
 */

const RELATIVE_LINK = /(?<!!)\[((?:[^\[\]]|\[[^\]]*\])+)\]\(([^)\s]+)(?:\s+"[^"]*")?\)/g;

function isRepoPath(target: string): boolean {
  return !/^(?:[a-z][a-z0-9+.-]*:|#|\/)/i.test(target);
}

/** One line of prose (never a fenced code line) with its repository links unlinked. */
export function unlinkRepoPaths(line: string): string {
  return line.replace(RELATIVE_LINK, (whole, text: string, target: string) =>
    isRepoPath(target) ? text : whole,
  );
}

/**
 * One line of prose with its repository links pointed at the repository on
 * GitHub instead: `[x](docs/a.md)` -> `[x](<repo>/blob/main/docs/a.md)`.
 * For a file at the repository root (CHANGELOG.md), whose relative links are
 * relative to the root.
 */
export function linkRepoPathsTo(line: string, repoUrl: string, branch = 'main'): string {
  return line.replace(RELATIVE_LINK, (whole, text: string, target: string) => {
    if (!isRepoPath(target)) return whole;
    const path = target.replace(/^\.\//, '');
    if (path.startsWith('../')) return text;
    return `[${text}](${repoUrl}/blob/${branch}/${path})`;
  });
}
