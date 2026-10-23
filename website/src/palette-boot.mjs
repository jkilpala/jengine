// Inline <head> script, run before first paint so a stored palette never
// flashes Dusk first. A string because it is inlined, not bundled; used by
// astro.config.mjs (docs pages) and src/layouts/Site.astro (designed pages).
// `?palette=sabers` shows a palette for that page without storing it (for
// sharing a link, and for screenshots). The key and ids must match
// src/palettes.ts.
export const PALETTE_BOOT =
	"(function(){var ok=function(p){return p==='sabers'||p==='genom'};var p=null;try{p=new URLSearchParams(location.search).get('palette')}catch(e){}if(!ok(p)&&p!=='dusk'){try{p=localStorage.getItem('jx-palette')}catch(e){}}if(ok(p))document.documentElement.dataset.palette=p;})();";
