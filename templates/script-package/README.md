# {{PACKAGE_NAME}}

A script package for jEngine.

## Structure

```
{{PACKAGE_NAME}}/
├── package.toml        — Package manifest
├── scripts/            — Lua/JS scripts
│   └── example.lua
└── README.md
```

## Usage

Add to your `project.jengine`:

```json
{
  "packages": [
    {
      "name": "{{PACKAGE_NAME}}",
      "source": { "type": "path", "path": "packages/{{PACKAGE_NAME}}" }
    }
  ]
}
```

Scripts are installed to `assets/packages/{{PACKAGE_NAME}}/scripts/` when the project resolves packages.
Reference them in `script` components as `packages/{{PACKAGE_NAME}}/scripts/example.lua`, or `require`
them as modules. A required module is cached for the whole session, not per scene, so reset its state in
`init`.

Scripts call the engine through namespaces (`Transform.get_position(e)`); JavaScript scripts use the flat
names and run on desktop builds only. Android builds run Lua only.
