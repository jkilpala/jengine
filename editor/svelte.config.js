/** @type {import("@sveltejs/vite-plugin-svelte").SvelteConfig} */
export default {
  onwarn(warning, handler) {
    // Suppress label-association warnings — this is a Tauri desktop editor, not a public website.
    if (warning.code === 'a11y_label_has_associated_control') return;
    handler(warning);
  },
}
