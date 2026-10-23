import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import { PALETTE_BOOT } from './src/palette-boot.mjs';

export default defineConfig({
  site: 'https://jengine.hirakame.com',
  server: { host: '0.0.0.0' },
  vite: {
    server: {
      allowedHosts: ['.trycloudflare.com'],
    },
  },
  integrations: [
    starlight({
      title: 'jEngine',
      description: 'A 2D game engine in Rust, scripted in Lua and JavaScript, with a desktop editor.',
      // No image logo: the striped sun beside the title is drawn in CSS
      // (custom.css, .site-title::before) so it follows the palette.
      head: [{ tag: 'script', content: PALETTE_BOOT }],
      components: {
        ThemeSelect: './src/components/ThemeSelect.astro',
        SocialIcons: './src/components/SocialIcons.astro',
      },
      expressiveCode: {
        themes: ['synthwave-84', 'catppuccin-latte'],
        styleOverrides: {
          codeBackground: 'var(--jx-code-bg)',
          borderColor: 'var(--jx-line)',
          borderRadius: '4px',
          codeFontFamily: 'var(--jx-mono)',
          uiFontFamily: 'var(--jx-body)',
          frames: {
            editorTabBarBackground: 'var(--jx-panel)',
            editorActiveTabBackground: 'var(--jx-code-bg)',
            editorActiveTabIndicatorTopColor: 'var(--jx-accent)',
            terminalTitlebarBackground: 'var(--jx-panel)',
            terminalBackground: 'var(--jx-code-bg)',
          },
        },
      },
      social: [
        { icon: 'github', label: 'GitHub', href: 'https://github.com/jkilpala/jengine' },
      ],
      customCss: [
        '@fontsource/ibm-plex-sans/400.css',
        '@fontsource/ibm-plex-sans/500.css',
        '@fontsource/ibm-plex-sans/600.css',
        '@fontsource/ibm-plex-sans/400-italic.css',
        '@fontsource/ibm-plex-mono/400.css',
        '@fontsource/ibm-plex-mono/500.css',
        '@fontsource/dela-gothic-one/400.css',
        './src/styles/palettes.css',
        './src/styles/custom.css',
      ],
      editLink: {
        baseUrl: 'https://github.com/jkilpala/jengine/edit/main/website/',
      },
      sidebar: [
        {
          label: 'Getting Started',
          items: [
            { label: 'Installation', slug: 'getting-started/installation' },
            { label: 'Your First Project', slug: 'getting-started/first-project' },
            { label: 'Editor Overview', slug: 'getting-started/editor-overview' },
            { label: 'Versioning & Stability', slug: 'getting-started/versioning' },
          ],
        },
        {
          label: 'Guides',
          items: [
            { label: 'ECS & Components', slug: 'guides/ecs' },
            { label: 'Scripting (Lua & JS)', slug: 'guides/scripting' },
            { label: 'Batch Scripts', slug: 'guides/batch-scripts' },
            { label: 'Behavior Graph', slug: 'guides/behavior-graph' },
            { label: 'Best Practices & Architecture', slug: 'guides/best-practices' },
            { label: 'Physics', slug: 'guides/physics' },
            { label: 'Audio', slug: 'guides/audio' },
            { label: 'Animation', slug: 'guides/animation' },
            { label: 'Tilemaps', slug: 'guides/tilemaps' },
            { label: 'Particles', slug: 'guides/particles' },
            { label: 'UI System', slug: 'guides/ui' },
            { label: 'Input System', slug: 'guides/input' },
            { label: 'Scene Management', slug: 'guides/scenes' },
            { label: 'Parallax Scrolling', slug: 'guides/parallax' },
            { label: 'Custom Rust Components & Packages', slug: 'guides/packages' },
            { label: 'Web Portals', slug: 'guides/web-portals' },
          ],
        },
        {
          label: 'Servers',
          items: [
            { label: 'Game Servers', slug: 'servers/overview' },
            { label: 'Hosting & Deploying', slug: 'servers/hosting' },
            { label: 'Raspberry Pi Host', slug: 'servers/raspberry-pi' },
            { label: 'Console & Live Ops', slug: 'servers/console' },
          ],
        },
        {
          // Generated from packages/*/README.md or src/lib.rs by
          // scripts/extract-packages.ts - one page per package.
          label: 'Packages',
          collapsed: true,
          autogenerate: { directory: 'packages' },
        },
        {
          label: 'API Reference',
          items: [
            { label: 'Scripting API', slug: 'api-reference/scripting-api' },
            { label: 'Components Reference', slug: 'api-reference/components' },
            { label: 'Scene Format', slug: 'api-reference/scene-format' },
            { label: 'Rust API (rustdoc)', link: '/rustdoc/jengine_core/' },
          ],
        },
        {
          label: 'Features',
          items: [
            { label: 'Overview', slug: 'features/overview' },
          ],
        },
        {
          label: 'Architecture',
          items: [
            { label: 'Engine Overview', slug: 'architecture/overview' },
            { label: 'Cross-Platform', slug: 'architecture/cross-platform' },
          ],
        },
        {
          label: 'AI / Agents',
          items: [
            { label: 'Overview', slug: 'ai/overview' },
            { label: 'Agent Guide', slug: 'ai/agent-guide' },
            { label: 'API Specification', slug: 'ai/api-specification' },
            { label: 'Example Projects', slug: 'ai/examples' },
            { label: 'Project Templates', slug: 'ai/templates' },
          ],
        },
        {
          // Generated from the root CHANGELOG.md by scripts/sync-changelog.ts.
          label: 'Releases',
          items: [
            { label: 'Changelog', slug: 'changelog' },
          ],
        },
      ],
    }),
  ],
});
