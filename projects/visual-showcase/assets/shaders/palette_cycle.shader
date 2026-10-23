{
 "meta": {
  "name": "Palette cycle",
  "description": "Pixel art that animates by turning its palette, as 16-bit water did: the texture's red channel is an index 0..7 into eight colours kept in its own last row, and the index is shifted by whole steps over time. The `cycle` parameter is (steps per second, the palette row's v, one texel's u, unused).",
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
    "type": "UvInput"
   },
   "position": [
    190,
    0
   ]
  },
  {
   "id": 2,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    380,
    0
   ]
  },
  {
   "id": 3,
   "node_type": {
    "type": "FloatConstant",
    "value": 8.0
   },
   "position": [
    570,
    0
   ]
  },
  {
   "id": 4,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    760,
    0
   ]
  },
  {
   "id": 5,
   "node_type": {
    "type": "Floor"
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
    "name": "cycle"
   },
   "position": [
    1140,
    0
   ]
  },
  {
   "id": 7,
   "node_type": {
    "type": "Split"
   },
   "position": [
    1330,
    0
   ]
  },
  {
   "id": 8,
   "node_type": {
    "type": "TimeInput"
   },
   "position": [
    0,
    150
   ]
  },
  {
   "id": 9,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    190,
    150
   ]
  },
  {
   "id": 10,
   "node_type": {
    "type": "Floor"
   },
   "position": [
    380,
    150
   ]
  },
  {
   "id": 11,
   "node_type": {
    "type": "Subtract"
   },
   "position": [
    570,
    150
   ]
  },
  {
   "id": 12,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.125
   },
   "position": [
    760,
    150
   ]
  },
  {
   "id": 13,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    950,
    150
   ]
  },
  {
   "id": 14,
   "node_type": {
    "type": "Floor"
   },
   "position": [
    1140,
    150
   ]
  },
  {
   "id": 15,
   "node_type": {
    "type": "FloatConstant",
    "value": 8.0
   },
   "position": [
    1330,
    150
   ]
  },
  {
   "id": 16,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    0,
    300
   ]
  },
  {
   "id": 17,
   "node_type": {
    "type": "Subtract"
   },
   "position": [
    190,
    300
   ]
  },
  {
   "id": 18,
   "node_type": {
    "type": "Combine",
    "components": 2
   },
   "position": [
    380,
    300
   ]
  },
  {
   "id": 19,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.5
   },
   "position": [
    570,
    300
   ]
  },
  {
   "id": 20,
   "node_type": {
    "type": "Add"
   },
   "position": [
    760,
    300
   ]
  },
  {
   "id": 21,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    950,
    300
   ]
  },
  {
   "id": 22,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    1140,
    300
   ]
  },
  {
   "id": 23,
   "node_type": {
    "type": "Split"
   },
   "position": [
    1330,
    300
   ]
  },
  {
   "id": 24,
   "node_type": {
    "type": "Combine",
    "components": 4
   },
   "position": [
    0,
    450
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
   "from_pin": 1,
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
   "to_node": 5,
   "to_pin": 0
  },
  {
   "from_node": 6,
   "from_pin": 0,
   "to_node": 7,
   "to_pin": 0
  },
  {
   "from_node": 8,
   "from_pin": 0,
   "to_node": 9,
   "to_pin": 0
  },
  {
   "from_node": 7,
   "from_pin": 0,
   "to_node": 9,
   "to_pin": 1
  },
  {
   "from_node": 9,
   "from_pin": 0,
   "to_node": 10,
   "to_pin": 0
  },
  {
   "from_node": 5,
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
   "to_node": 14,
   "to_pin": 0
  },
  {
   "from_node": 14,
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
   "from_node": 11,
   "from_pin": 0,
   "to_node": 17,
   "to_pin": 0
  },
  {
   "from_node": 16,
   "from_pin": 0,
   "to_node": 17,
   "to_pin": 1
  },
  {
   "from_node": 17,
   "from_pin": 0,
   "to_node": 20,
   "to_pin": 0
  },
  {
   "from_node": 19,
   "from_pin": 0,
   "to_node": 20,
   "to_pin": 1
  },
  {
   "from_node": 20,
   "from_pin": 0,
   "to_node": 21,
   "to_pin": 0
  },
  {
   "from_node": 7,
   "from_pin": 2,
   "to_node": 21,
   "to_pin": 1
  },
  {
   "from_node": 21,
   "from_pin": 0,
   "to_node": 18,
   "to_pin": 0
  },
  {
   "from_node": 7,
   "from_pin": 1,
   "to_node": 18,
   "to_pin": 1
  },
  {
   "from_node": 18,
   "from_pin": 0,
   "to_node": 22,
   "to_pin": 0
  },
  {
   "from_node": 22,
   "from_pin": 0,
   "to_node": 23,
   "to_pin": 0
  },
  {
   "from_node": 23,
   "from_pin": 0,
   "to_node": 24,
   "to_pin": 0
  },
  {
   "from_node": 23,
   "from_pin": 1,
   "to_node": 24,
   "to_pin": 1
  },
  {
   "from_node": 23,
   "from_pin": 2,
   "to_node": 24,
   "to_pin": 2
  },
  {
   "from_node": 2,
   "from_pin": 4,
   "to_node": 24,
   "to_pin": 3
  },
  {
   "from_node": 24,
   "from_pin": 0,
   "to_node": 0,
   "to_pin": 0
  }
 ],
 "next_id": 25
}
