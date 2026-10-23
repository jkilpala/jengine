{
  "meta": {
    "name": "Parameter Tint",
    "description": "The sampled texture times a `tint` MaterialParam - a vec4 the script sets per material instance with set_material_uniform_vec4. Three sprites share this material and one texture and differ only in that number. Before Sep 2026 MaterialComponent.uniforms was written by four script functions and read by nothing.",
    "author": "jEngine"
  },
  "nodes": [
    {
      "id": 0,
      "node_type": {
        "type": "FragmentOutput"
      },
      "position": [
        800,
        200
      ]
    },
    {
      "id": 1,
      "node_type": {
        "type": "UvInput"
      },
      "position": [
        0,
        100
      ]
    },
    {
      "id": 2,
      "node_type": {
        "type": "TextureSample"
      },
      "position": [
        200,
        100
      ]
    },
    {
      "id": 3,
      "node_type": {
        "type": "MaterialParam",
        "name": "tint"
      },
      "position": [
        200,
        300
      ]
    },
    {
      "id": 4,
      "node_type": {
        "type": "Multiply"
      },
      "position": [
        500,
        200
      ]
    }
  ],
  "edges": [
    {
      "from_node": 1,
      "from_pin": 0,
      "to_node": 2,
      "to_pin": 0
    },
    {
      "from_node": 2,
      "from_pin": 0,
      "to_node": 4,
      "to_pin": 0
    },
    {
      "from_node": 3,
      "from_pin": 0,
      "to_node": 4,
      "to_pin": 1
    },
    {
      "from_node": 4,
      "from_pin": 0,
      "to_node": 0,
      "to_pin": 0
    }
  ],
  "next_id": 5
}
