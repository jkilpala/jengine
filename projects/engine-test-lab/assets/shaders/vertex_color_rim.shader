{
  "meta": {
    "name": "Vertex Colour Rim",
    "description": "Reads the sprite's own vertex colour - the one per-entity value a graph can see - and adds an emissive rim once it passes a threshold. Before NodeType::VertexColorInput (Sep 2026) a graph could read UV, time, position, the sampled texture and constants, none of which a game knows anything about, which is why no game had ever used a graph material. MaterialComponent.uniforms looks like the answer and is inert - see docs/parity-gaps.md.",
    "author": "jEngine"
  },
  "nodes": [
    {
      "id": 0,
      "node_type": {
        "type": "FragmentOutput"
      },
      "position": [
        1200,
        300
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
        "type": "VertexColorInput"
      },
      "position": [
        0,
        320
      ]
    },
    {
      "id": 4,
      "node_type": {
        "type": "Multiply"
      },
      "position": [
        420,
        160
      ]
    },
    {
      "id": 5,
      "node_type": {
        "type": "Split"
      },
      "position": [
        200,
        420
      ]
    },
    {
      "id": 6,
      "node_type": {
        "type": "Max"
      },
      "position": [
        400,
        420
      ]
    },
    {
      "id": 7,
      "node_type": {
        "type": "Max"
      },
      "position": [
        560,
        440
      ]
    },
    {
      "id": 8,
      "node_type": {
        "type": "FloatConstant",
        "value": 0.55
      },
      "position": [
        400,
        560
      ]
    },
    {
      "id": 9,
      "node_type": {
        "type": "FloatConstant",
        "value": 0.95
      },
      "position": [
        400,
        640
      ]
    },
    {
      "id": 10,
      "node_type": {
        "type": "SmoothStep"
      },
      "position": [
        720,
        500
      ]
    },
    {
      "id": 11,
      "node_type": {
        "type": "Multiply"
      },
      "position": [
        900,
        420
      ]
    },
    {
      "id": 12,
      "node_type": {
        "type": "Multiply"
      },
      "position": [
        1040,
        440
      ]
    },
    {
      "id": 13,
      "node_type": {
        "type": "Add"
      },
      "position": [
        1040,
        260
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
      "from_node": 3,
      "from_pin": 0,
      "to_node": 5,
      "to_pin": 0
    },
    {
      "from_node": 5,
      "from_pin": 0,
      "to_node": 6,
      "to_pin": 0
    },
    {
      "from_node": 5,
      "from_pin": 1,
      "to_node": 6,
      "to_pin": 1
    },
    {
      "from_node": 6,
      "from_pin": 0,
      "to_node": 7,
      "to_pin": 0
    },
    {
      "from_node": 5,
      "from_pin": 2,
      "to_node": 7,
      "to_pin": 1
    },
    {
      "from_node": 8,
      "from_pin": 0,
      "to_node": 10,
      "to_pin": 0
    },
    {
      "from_node": 9,
      "from_pin": 0,
      "to_node": 10,
      "to_pin": 1
    },
    {
      "from_node": 7,
      "from_pin": 0,
      "to_node": 10,
      "to_pin": 2
    },
    {
      "from_node": 3,
      "from_pin": 0,
      "to_node": 11,
      "to_pin": 0
    },
    {
      "from_node": 10,
      "from_pin": 0,
      "to_node": 11,
      "to_pin": 1
    },
    {
      "from_node": 11,
      "from_pin": 0,
      "to_node": 12,
      "to_pin": 0
    },
    {
      "from_node": 2,
      "from_pin": 4,
      "to_node": 12,
      "to_pin": 1
    },
    {
      "from_node": 4,
      "from_pin": 0,
      "to_node": 13,
      "to_pin": 0
    },
    {
      "from_node": 12,
      "from_pin": 0,
      "to_node": 13,
      "to_pin": 1
    },
    {
      "from_node": 13,
      "from_pin": 0,
      "to_node": 0,
      "to_pin": 0
    }
  ],
  "next_id": 14
}
