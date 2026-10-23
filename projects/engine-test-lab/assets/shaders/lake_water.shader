{
 "meta": {
  "name": "Lake water",
  "description": "Water that reflects whatever stands above it, wherever the camera is: each pixel reads the scene at its mirror image across the surface - a World To Screen node on (x, 2 * surface - y), minus its own Screen Position - bent by small waves, by up to three ripple rings the script drops, and by the wind. Darker and bluer toward the viewer, with the moon's glint broken across it. `lake` is (surface y, moon x, wind 0..1, unused); `ra`..`rc` are ripples (x, y, age in seconds, strength).",
  "author": "jEngine showcase"
 },
 "nodes": [
  {
   "id": 0,
   "node_type": {
    "type": "FragmentOutput"
   },
   "position": [
    1600,
    300
   ]
  },
  {
   "id": 1,
   "node_type": {
    "type": "PositionInput"
   },
   "position": [
    190,
    0
   ]
  },
  {
   "id": 2,
   "node_type": {
    "type": "ScreenPosition"
   },
   "position": [
    380,
    0
   ]
  },
  {
   "id": 3,
   "node_type": {
    "type": "TimeInput"
   },
   "position": [
    570,
    0
   ]
  },
  {
   "id": 4,
   "node_type": {
    "type": "MaterialParam",
    "name": "lake"
   },
   "position": [
    760,
    0
   ]
  },
  {
   "id": 5,
   "node_type": {
    "type": "MaterialParam",
    "name": "ripple_a"
   },
   "position": [
    950,
    0
   ]
  },
  {
   "id": 6,
   "node_type": {
    "type": "MaterialParam",
    "name": "ripple_b"
   },
   "position": [
    1140,
    0
   ]
  },
  {
   "id": 7,
   "node_type": {
    "type": "MaterialParam",
    "name": "ripple_c"
   },
   "position": [
    1330,
    0
   ]
  },
  {
   "id": 8,
   "node_type": {
    "type": "CustomGlsl",
    "code": "let d = max(lake.x - world.y, 0.0);\nlet wind = lake.z;\n// Small waves: a few sines, crossing, finer and stronger toward the viewer.\nvar w = sin(world.x * 0.021 + time * 0.9 + sin(world.y * 0.13 + time * 0.5) * 1.5);\nw = w + 0.6 * sin(world.x * 0.047 - time * 1.3 + world.y * 0.31);\nw = w + 0.35 * sin(world.y * 0.9 + time * 2.1 + sin(world.x * 0.011) * 3.0);\nvar dx = w * (0.6 + 2.4 * wind) * (0.2 + d * 0.012);\nvar dy = w * (0.3 + 1.2 * wind) * (0.1 + d * 0.004);\nring = 0.0;\n{\n    let age = ra.z;\n    if (age > 0.0 && age < 5.0 && ra.w > 0.0) {\n        let q = (world - ra.xy) * vec2<f32>(1.0, 3.2);\n        let dist = length(q);\n        let front = age * 150.0;\n        let env = exp(-age * 0.8) * smoothstep(front + 12.0, front - 50.0, dist) * ra.w;\n        let wave = sin(dist * 0.12 - age * 10.0) * env;\n        let dir = q / max(dist, 1.0);\n        dx = dx + dir.x * wave * 10.0;\n        dy = dy + dir.y * wave * 3.0;\n        ring = ring + wave;\n    }\n}\n{\n    let age = rb.z;\n    if (age > 0.0 && age < 5.0 && rb.w > 0.0) {\n        let q = (world - rb.xy) * vec2<f32>(1.0, 3.2);\n        let dist = length(q);\n        let front = age * 150.0;\n        let env = exp(-age * 0.8) * smoothstep(front + 12.0, front - 50.0, dist) * rb.w;\n        let wave = sin(dist * 0.12 - age * 10.0) * env;\n        let dir = q / max(dist, 1.0);\n        dx = dx + dir.x * wave * 10.0;\n        dy = dy + dir.y * wave * 3.0;\n        ring = ring + wave;\n    }\n}\n{\n    let age = rc.z;\n    if (age > 0.0 && age < 5.0 && rc.w > 0.0) {\n        let q = (world - rc.xy) * vec2<f32>(1.0, 3.2);\n        let dist = length(q);\n        let front = age * 150.0;\n        let env = exp(-age * 0.8) * smoothstep(front + 12.0, front - 50.0, dist) * rc.w;\n        let wave = sin(dist * 0.12 - age * 10.0) * env;\n        let dir = q / max(dist, 1.0);\n        dx = dx + dir.x * wave * 10.0;\n        dy = dy + dir.y * wave * 3.0;\n        ring = ring + wave;\n    }\n}\n// The point as far above the surface as this one is below it.\nmirror = vec2<f32>(world.x + dx, 2.0 * lake.x - world.y + dy);\ndepth = d;\n",
    "inputs": [
     {
      "name": "world",
      "pin_type": "Vec2"
     },
     {
      "name": "time",
      "pin_type": "Float"
     },
     {
      "name": "lake",
      "pin_type": "Vec4"
     },
     {
      "name": "ra",
      "pin_type": "Vec4"
     },
     {
      "name": "rb",
      "pin_type": "Vec4"
     },
     {
      "name": "rc",
      "pin_type": "Vec4"
     }
    ],
    "outputs": [
     {
      "name": "mirror",
      "pin_type": "Vec2"
     },
     {
      "name": "depth",
      "pin_type": "Float"
     },
     {
      "name": "ring",
      "pin_type": "Float"
     }
    ]
   },
   "position": [
    0,
    150
   ]
  },
  {
   "id": 9,
   "node_type": {
    "type": "WorldToScreen"
   },
   "position": [
    190,
    150
   ]
  },
  {
   "id": 10,
   "node_type": {
    "type": "Subtract"
   },
   "position": [
    380,
    150
   ]
  },
  {
   "id": 11,
   "node_type": {
    "type": "SceneColor"
   },
   "position": [
    570,
    150
   ]
  },
  {
   "id": 12,
   "node_type": {
    "type": "CustomGlsl",
    "code": "let k = clamp(depth / 280.0, 0.0, 1.0);\n// A lake mirrors best far off, and shows its own dark blue up close.\nlet tint = mix(vec3<f32>(0.80, 0.82, 0.94), vec3<f32>(0.16, 0.22, 0.34), k);\nvar c = scene.rgb * tint;\n// The moon's glint: a broken column of light under it, wider toward us.\nlet gx = world.x - lake.y;\nlet spread = 8.0 + depth * 0.16;\nlet column = exp(-gx * gx / (2.0 * spread * spread));\n// Glints: short flecks scattered at random over a grid of cells, each\n// flaring and fading on its own clock - a hash per cell, not a lattice.\nlet cell = floor(world / vec2<f32>(6.0, 2.0));\nlet h = fract(sin(dot(cell, vec2<f32>(12.9898, 78.233))) * 43758.547);\nlet flare = max(sin(time * (1.5 + 2.5 * h) + h * 60.0), 0.0);\nlet sparkle = step(0.9, h) * flare * flare;\nc = c + vec3<f32>(0.95, 0.95, 1.1) * sparkle * column * (1.5 + 2.0 * lake.z);\n// Ripple crests catch the sky.\nc = c + vec3<f32>(0.30, 0.30, 0.45) * max(ring, 0.0) * 0.7;\n// A bright thread where the water meets the far shore.\nc = c + vec3<f32>(0.7, 0.55, 0.55) * exp(-depth * 0.5) * 0.6;\ncolor = vec4<f32>(c, 1.0);\n",
    "inputs": [
     {
      "name": "scene",
      "pin_type": "Vec4"
     },
     {
      "name": "depth",
      "pin_type": "Float"
     },
     {
      "name": "ring",
      "pin_type": "Float"
     },
     {
      "name": "world",
      "pin_type": "Vec2"
     },
     {
      "name": "time",
      "pin_type": "Float"
     },
     {
      "name": "lake",
      "pin_type": "Vec4"
     }
    ],
    "outputs": [
     {
      "name": "color",
      "pin_type": "Vec4"
     }
    ]
   },
   "position": [
    760,
    150
   ]
  }
 ],
 "edges": [
  {
   "from_node": 1,
   "from_pin": 0,
   "to_node": 8,
   "to_pin": 0
  },
  {
   "from_node": 3,
   "from_pin": 0,
   "to_node": 8,
   "to_pin": 1
  },
  {
   "from_node": 4,
   "from_pin": 0,
   "to_node": 8,
   "to_pin": 2
  },
  {
   "from_node": 5,
   "from_pin": 0,
   "to_node": 8,
   "to_pin": 3
  },
  {
   "from_node": 6,
   "from_pin": 0,
   "to_node": 8,
   "to_pin": 4
  },
  {
   "from_node": 7,
   "from_pin": 0,
   "to_node": 8,
   "to_pin": 5
  },
  {
   "from_node": 8,
   "from_pin": 0,
   "to_node": 9,
   "to_pin": 0
  },
  {
   "from_node": 9,
   "from_pin": 0,
   "to_node": 10,
   "to_pin": 0
  },
  {
   "from_node": 2,
   "from_pin": 0,
   "to_node": 10,
   "to_pin": 1
  },
  {
   "from_node": 10,
   "from_pin": 0,
   "to_node": 11,
   "to_pin": 0
  },
  {
   "from_node": 11,
   "from_pin": 0,
   "to_node": 12,
   "to_pin": 0
  },
  {
   "from_node": 8,
   "from_pin": 1,
   "to_node": 12,
   "to_pin": 1
  },
  {
   "from_node": 8,
   "from_pin": 2,
   "to_node": 12,
   "to_pin": 2
  },
  {
   "from_node": 1,
   "from_pin": 0,
   "to_node": 12,
   "to_pin": 3
  },
  {
   "from_node": 3,
   "from_pin": 0,
   "to_node": 12,
   "to_pin": 4
  },
  {
   "from_node": 4,
   "from_pin": 0,
   "to_node": 12,
   "to_pin": 5
  },
  {
   "from_node": 12,
   "from_pin": 0,
   "to_node": 0,
   "to_pin": 0
  }
 ],
 "next_id": 13
}
