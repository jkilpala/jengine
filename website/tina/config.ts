import { defineConfig } from 'tinacms';

export default defineConfig({
  // Use local filesystem mode for development
  // For production self-hosted mode, configure a database backend later
  branch: '',
  clientId: '',
  token: '',

  build: {
    outputFolder: 'admin',
    publicFolder: 'public',
  },

  media: {
    tina: {
      mediaRoot: 'images',
      publicFolder: 'public',
    },
  },

  schema: {
    collections: [
      {
        name: 'docs',
        label: 'Documentation',
        path: 'src/content/docs',
        format: 'mdx',
        fields: [
          {
            type: 'string',
            name: 'title',
            label: 'Title',
            required: true,
            isTitle: true,
          },
          {
            type: 'string',
            name: 'description',
            label: 'Description',
          },
          {
            type: 'number',
            name: 'order',
            label: 'Sort Order',
            description: 'Controls the order in the sidebar (lower = first)',
          },
          {
            type: 'object',
            name: 'sidebar',
            label: 'Sidebar',
            fields: [
              {
                type: 'string',
                name: 'label',
                label: 'Sidebar Label',
                description: 'Override the title shown in the sidebar',
              },
              {
                type: 'number',
                name: 'order',
                label: 'Order',
              },
              {
                type: 'string',
                name: 'badge',
                label: 'Badge Text',
                description: 'Optional badge shown next to the label (e.g. "New", "Beta")',
              },
            ],
          },
          {
            type: 'rich-text',
            name: 'body',
            label: 'Body',
            isBody: true,
            templates: [
              {
                name: 'Callout',
                label: 'Callout Box',
                fields: [
                  {
                    type: 'string',
                    name: 'type',
                    label: 'Type',
                    options: ['note', 'tip', 'caution', 'danger'],
                  },
                  {
                    type: 'string',
                    name: 'title',
                    label: 'Title',
                  },
                  {
                    type: 'rich-text',
                    name: 'children',
                    label: 'Content',
                  },
                ],
              },
              {
                name: 'CodeBlock',
                label: 'Code Block',
                fields: [
                  {
                    type: 'string',
                    name: 'language',
                    label: 'Language',
                    options: ['lua', 'javascript', 'rust', 'json', 'bash', 'glsl'],
                  },
                  {
                    type: 'string',
                    name: 'title',
                    label: 'Filename / Title',
                  },
                  {
                    type: 'string',
                    name: 'code',
                    label: 'Code',
                    ui: {
                      component: 'textarea',
                    },
                  },
                ],
              },
            ],
          },
        ],
      },
    ],
  },
});
