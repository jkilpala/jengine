// The Help menu's addresses and the About box's credits, against what they
// point at: every documentation route is a page of the site in `website/`,
// and every credit names a crate, npm package or asset the third-party list
// (tools/release/third_party_notices.mjs) says ships.
import { existsSync, readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { LINKS, REPO, SITE, SITE_ROUTES } from '../src/lib/links';
import { CREDITS, creditDetails } from '../src/lib/credits';

const ROOT = resolve(__dirname, '../..');
const DOCS = resolve(ROOT, 'website/src/content/docs');

/** `/api-reference/components/` -> the content file Starlight serves there. */
function pageFor(route: string): string | null {
  const slug = route.replace(/^\/|\/$/g, '');
  for (const candidate of [`${slug}.md`, `${slug}.mdx`, `${slug}/index.md`, `${slug}/index.mdx`]) {
    if (existsSync(resolve(DOCS, candidate))) return candidate;
  }
  return null;
}

describe('help links', () => {
  it('point at pages the site has', () => {
    expect(SITE_ROUTES.filter((r) => pageFor(r) === null)).toEqual([]);
  });

  it('are all on the site or the repository, over https', () => {
    for (const url of Object.values(LINKS)) {
      expect(url.startsWith(`${SITE}/`) || url === REPO || url.startsWith(`${REPO}/`), url).toBe(true);
    }
    // Every site link is one of the checked routes (or the home page).
    for (const url of Object.values(LINKS).filter((u) => u.startsWith(SITE))) {
      const path = url.slice(SITE.length);
      expect(path === '/' || (SITE_ROUTES as readonly string[]).includes(path), url).toBe(true);
    }
  });

  it('use the site the docs are published at', () => {
    const astro = readFileSync(resolve(ROOT, 'website/astro.config.mjs'), 'utf8');
    expect(astro).toContain(`site: '${SITE}'`);
  });
});

describe('credits', () => {
  it('each name something that ships', () => {
    const missing = CREDITS.filter((c) => creditDetails(c) === null).map((c) => c.name);
    expect(missing).toEqual([]);
  });
});
