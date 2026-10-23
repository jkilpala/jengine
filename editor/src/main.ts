import { mount } from 'svelte'
import '@fontsource/ibm-plex-sans/400.css'
import '@fontsource/ibm-plex-sans/500.css'
import '@fontsource/ibm-plex-sans/600.css'
import '@fontsource/ibm-plex-mono/400.css'
import '@fontsource/ibm-plex-mono/500.css'
import '@fontsource/dela-gothic-one/400.css'
import './theme.css'
import './editor.css'
// Applies the chosen palette to <html> on import, before the first paint.
import './lib/stores/themeStore'
// Install the Fengari (real Lua 5.3 VM) runtime BEFORE any viewport script
// loads — this registers `globalThis.__jengine_lua_compile`, which the
// editor-viewport WASM picks up to bypass the brittle Lua\u2192JS transpiler.
// The runtime is host-agnostic (the same source ships in web builds), so the
// editor supplies the error sink that routes script errors to its Console.
import { logError } from './lib/stores/consoleStore'
;(globalThis as any).__jengine_script_error = (
  message: string,
  scriptPath?: string,
  scriptLine?: number,
) => logError(message, scriptPath, scriptLine)
// And tag what scripts print with where they printed it, for the Console's links.
;(globalThis as any).__jengine_log_locations = true
import './lib/lua-runtime'
import App from './App.svelte'

// Disable the default browser/WebView context menu so right-click works for the editor
document.addEventListener('contextmenu', (e) => e.preventDefault());

const target = document.getElementById('app')!;
try {
  mount(App, { target });
} catch (e: unknown) {
  // Surface any silent Svelte mount error directly in the page
  target.style.cssText = 'background:var(--ed-base);color:var(--ed-err);padding:32px;font-family:monospace;white-space:pre;height:100vh';
  target.textContent = `jEngine Editor - mount() failed:\n\n${String(e)}\n\n${(e instanceof Error ? e.stack : '')}`;
}
