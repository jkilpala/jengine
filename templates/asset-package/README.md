# {{PACKAGE_NAME}}

An asset package for jEngine.

## Structure

```
{{PACKAGE_NAME}}/
├── package.toml        — Package manifest
├── sprites/            — PNG/JPG textures
├── sounds/             — OGG/WAV/MP3 audio
├── prefabs/            — Prefab definitions (.prefab)
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

Assets are installed to `assets/packages/{{PACKAGE_NAME}}/` when the project resolves packages.
Reference them by their installed path, e.g. `packages/{{PACKAGE_NAME}}/sprites/hero.png`.

- A prefab's `texture_id` must use the installed path too, not a path inside this package.
- A sound plays only if the scene lists it in its top-level `"audio_refs"` array.
- Keep ready-made sprite sheets with their `.atlas` beside them and name the atlas in a scene's
  `atlas_refs`; only `assets/sprites/` is re-packed by the asset pipeline.
