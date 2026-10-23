import { defineConfig, type Plugin } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

/**
 * A built editor imports its WASM viewport at run time rather than bundling
 * it: an installed editor loads a project's own viewport build (one with its
 * Rust packages linked in) without being rebuilt, and that build's JS glue
 * differs from the stock one, so the glue is loaded with it. Every import of
 * `viewport-wasm/jengine_editor_viewport.js` becomes an import of the
 * editor's `jengine-viewport` scheme (`editor/src-tauri/src/viewport_module.rs`),
 * which serves the project's module or the stock one. `vite dev` - `cargo
 * tauri dev` - still imports the checkout's folder, as it always did.
 */
function viewportAtRunTime(): Plugin {
  // WebView2 (Windows) and Android reach a custom scheme as
  // `http://<scheme>.localhost/`; WKWebView and WebKitGTK as `<scheme>://localhost/`.
  const platform = process.env.TAURI_ENV_PLATFORM ?? (process.platform === 'win32' ? 'windows' : process.platform)
  const base = platform === 'windows' || platform === 'android'
    ? 'http://jengine-viewport.localhost'
    : 'jengine-viewport://localhost'
  return {
    name: 'jengine-viewport-at-run-time',
    apply: 'build',
    enforce: 'pre',
    resolveId(source) {
      if (source.endsWith('viewport-wasm/jengine_editor_viewport.js')) {
        return { id: `${base}/jengine_editor_viewport.js`, external: true }
      }
      return null
    },
  }
}

// https://vite.dev/config/
export default defineConfig({
  plugins: [viewportAtRunTime(), svelte()],
})
