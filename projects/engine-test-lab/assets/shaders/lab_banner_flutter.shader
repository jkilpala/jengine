{
 "meta": {
  "name": "Banner flutter",
  "description": "A wave travelling along a static mesh grid, pinned at the pole (times uv.x): every vertex of the mesh moves, where a sprite has only its four corners. A VertexOffset node.",
  "author": "jEngine"
 },
 "nodes": [
  {
   "id": 0,
   "node_type": {
    "type": "FragmentOutput"
   },
   "position": [
    1400,
    200
   ]
  },
  {
   "id": 1,
   "node_type": {
    "type": "UvInput"
   },
   "position": [
    150,
    0
   ]
  },
  {
   "id": 2,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    300,
    0
   ]
  },
  {
   "id": 3,
   "node_type": {
    "type": "VertexColorInput"
   },
   "position": [
    450,
    0
   ]
  },
  {
   "id": 4,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    600,
    0
   ]
  },
  {
   "id": 5,
   "node_type": {
    "type": "TimeInput"
   },
   "position": [
    750,
    0
   ]
  },
  {
   "id": 6,
   "node_type": {
    "type": "FloatConstant",
    "value": 3.0
   },
   "position": [
    900,
    0
   ]
  },
  {
   "id": 7,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1050,
    0
   ]
  },
  {
   "id": 8,
   "node_type": {
    "type": "PositionInput"
   },
   "position": [
    1200,
    0
   ]
  },
  {
   "id": 9,
   "node_type": {
    "type": "Split"
   },
   "position": [
    0,
    120
   ]
  },
  {
   "id": 10,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.025
   },
   "position": [
    150,
    120
   ]
  },
  {
   "id": 11,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    300,
    120
   ]
  },
  {
   "id": 12,
   "node_type": {
    "type": "Subtract"
   },
   "position": [
    450,
    120
   ]
  },
  {
   "id": 13,
   "node_type": {
    "type": "Sin"
   },
   "position": [
    600,
    120
   ]
  },
  {
   "id": 14,
   "node_type": {
    "type": "Split"
   },
   "position": [
    750,
    120
   ]
  },
  {
   "id": 15,
   "node_type": {
    "type": "FloatConstant",
    "value": 22.0
   },
   "position": [
    900,
    120
   ]
  },
  {
   "id": 16,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1050,
    120
   ]
  },
  {
   "id": 17,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1200,
    120
   ]
  },
  {
   "id": 18,
   "node_type": {
    "type": "Vec2Constant",
    "value": [
     0.0,
     1.0
    ]
   },
   "position": [
    0,
    240
   ]
  },
  {
   "id": 19,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    150,
    240
   ]
  },
  {
   "id": 20,
   "node_type": {
    "type": "VertexOffset"
   },
   "position": [
    300,
    240
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
  },
  {
   "from_node": 5,
   "from_pin": 0,
   "to_node": 7,
   "to_pin": 0
  },
  {
   "from_node": 6,
   "from_pin": 0,
   "to_node": 7,
   "to_pin": 1
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
   "from_node": 7,
   "from_pin": 0,
   "to_node": 12,
   "to_pin": 0
  },
  {
   "from_node": 11,
   "from_pin": 0,
   "to_node": 12,
   "to_pin": 1
  },
  {
   "from_node": 12,
   "from_pin": 0,
   "to_node": 13,
   "to_pin": 0
  },
  {
   "from_node": 1,
   "from_pin": 0,
   "to_node": 14,
   "to_pin": 0
  },
  {
   "from_node": 13,
   "from_pin": 0,
   "to_node": 16,
   "to_pin": 0
  },
  {
   "from_node": 15,
   "from_pin": 0,
   "to_node": 16,
   "to_pin": 1
  },
  {
   "from_node": 16,
   "from_pin": 0,
   "to_node": 17,
   "to_pin": 0
  },
  {
   "from_node": 14,
   "from_pin": 0,
   "to_node": 17,
   "to_pin": 1
  },
  {
   "from_node": 18,
   "from_pin": 0,
   "to_node": 19,
   "to_pin": 0
  },
  {
   "from_node": 17,
   "from_pin": 0,
   "to_node": 19,
   "to_pin": 1
  },
  {
   "from_node": 19,
   "from_pin": 0,
   "to_node": 20,
   "to_pin": 0
  }
 ],
 "next_id": 21
}
