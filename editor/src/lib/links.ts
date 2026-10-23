/**
 * Every address the editor opens, in one place.
 *
 * The site paths are routes of the Starlight site in `website/`
 * (`src/content/docs/<path>`); `links.test.ts` checks each one still has its
 * page there, so a renamed page cannot leave a Help menu item pointing at a
 * 404.
 */

export const SITE = "https://jengine.hirakame.com";
export const REPO = "https://github.com/jkilpala/jengine";

/** A page of the documentation site, by its route. */
function site(path: string): string {
  return `${SITE}${path}`;
}

export const LINKS = {
  site: site("/"),
  gettingStarted: site("/getting-started/installation/"),
  firstProject: site("/getting-started/first-project/"),
  editorOverview: site("/getting-started/editor-overview/"),
  scriptingApi: site("/api-reference/scripting-api/"),
  components: site("/api-reference/components/"),
  packages: site("/packages/"),
  repo: REPO,
  releaseNotes: `${REPO}/blob/main/CHANGELOG.md`,
  reportIssue: `${REPO}/issues/new/choose`,
  license: `${REPO}/blob/main/LICENSE`,
  thirdPartyNotices: `${REPO}/blob/main/THIRD-PARTY-NOTICES.md`,
} as const;

/** The documentation site's routes the links above use (for the test). */
export const SITE_ROUTES = [
  "/getting-started/installation/",
  "/getting-started/first-project/",
  "/getting-started/editor-overview/",
  "/api-reference/scripting-api/",
  "/api-reference/components/",
  "/packages/",
] as const;

export const COPYRIGHT = "Copyright (c) 2026 Juha Kilpala and the jEngine contributors";
export const LICENSE_NAME = "MIT";
