{
  "meta": {
    "name": "Pulse Emission",
    "description": "Multiplies the sampled texture color by 1.0 + 0.5*sin(time) to produce a smooth pulsing brightness effect. Used by scenes/21-shader-graph.scene via materials/pulse_emission.material as the first end-to-end demo of the .shader -> .material -> runtime pipeline (Slice 13c).",
    "author": "jEngine"
  },
  "nodes": [
    { "id": 0, "node_type": { "type": "FragmentOutput" }, "position": [800.0, 200.0] },
    { "id": 1, "node_type": { "type": "UvInput" }, "position": [0.0, 100.0] },
    { "id": 2, "node_type": { "type": "TextureSample" }, "position": [200.0, 100.0] },
    { "id": 3, "node_type": { "type": "TimeInput" }, "position": [0.0, 300.0] },
    { "id": 4, "node_type": { "type": "Sin" }, "position": [200.0, 300.0] },
    { "id": 5, "node_type": { "type": "FloatConstant", "value": 0.5 }, "position": [200.0, 400.0] },
    { "id": 6, "node_type": { "type": "Multiply" }, "position": [400.0, 300.0] },
    { "id": 7, "node_type": { "type": "FloatConstant", "value": 1.0 }, "position": [400.0, 400.0] },
    { "id": 8, "node_type": { "type": "Add" }, "position": [600.0, 300.0] },
    { "id": 9, "node_type": { "type": "Brightness" }, "position": [600.0, 150.0] }
  ],
  "edges": [
    { "from_node": 1, "from_pin": 0, "to_node": 2, "to_pin": 0 },
    { "from_node": 3, "from_pin": 0, "to_node": 4, "to_pin": 0 },
    { "from_node": 4, "from_pin": 0, "to_node": 6, "to_pin": 0 },
    { "from_node": 5, "from_pin": 0, "to_node": 6, "to_pin": 1 },
    { "from_node": 6, "from_pin": 0, "to_node": 8, "to_pin": 0 },
    { "from_node": 7, "from_pin": 0, "to_node": 8, "to_pin": 1 },
    { "from_node": 2, "from_pin": 0, "to_node": 9, "to_pin": 0 },
    { "from_node": 8, "from_pin": 0, "to_node": 9, "to_pin": 1 },
    { "from_node": 9, "from_pin": 0, "to_node": 0, "to_pin": 0 }
  ],
  "next_id": 10
}
