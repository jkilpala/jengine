{
 "meta": {
  "name": "Heat Haze",
  "description": "What is behind the sprite, read back through a Scene Color node and pushed sideways by noise rising over time - the air above a fire. Strongest over the flames, nothing at the sprite's edges, so it has no outline.",
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
    "type": "Split"
   },
   "position": [
    380,
    0
   ]
  },
  {
   "id": 3,
   "node_type": {
    "type": "Vec2Constant",
    "value": [
     1.0,
     0.45
    ]
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
    "type": "TimeInput"
   },
   "position": [
    950,
    0
   ]
  },
  {
   "id": 6,
   "node_type": {
    "type": "Vec2Constant",
    "value": [
     0.0,
     0.32
    ]
   },
   "position": [
    1140,
    0
   ]
  },
  {
   "id": 7,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1330,
    0
   ]
  },
  {
   "id": 8,
   "node_type": {
    "type": "Add"
   },
   "position": [
    0,
    150
   ]
  },
  {
   "id": 9,
   "node_type": {
    "type": "Noise",
    "octaves": 3
   },
   "position": [
    190,
    150
   ]
  },
  {
   "id": 10,
   "node_type": {
    "type": "FloatConstant",
    "value": 7.0
   },
   "position": [
    380,
    150
   ]
  },
  {
   "id": 11,
   "node_type": {
    "type": "Vec2Constant",
    "value": [
     3.7,
     1.3
    ]
   },
   "position": [
    570,
    150
   ]
  },
  {
   "id": 12,
   "node_type": {
    "type": "Add"
   },
   "position": [
    760,
    150
   ]
  },
  {
   "id": 13,
   "node_type": {
    "type": "TimeInput"
   },
   "position": [
    950,
    150
   ]
  },
  {
   "id": 14,
   "node_type": {
    "type": "Vec2Constant",
    "value": [
     0.0,
     0.27
    ]
   },
   "position": [
    1140,
    150
   ]
  },
  {
   "id": 15,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1330,
    150
   ]
  },
  {
   "id": 16,
   "node_type": {
    "type": "Add"
   },
   "position": [
    0,
    300
   ]
  },
  {
   "id": 17,
   "node_type": {
    "type": "Noise",
    "octaves": 2
   },
   "position": [
    190,
    300
   ]
  },
  {
   "id": 18,
   "node_type": {
    "type": "FloatConstant",
    "value": 5.0
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
    "type": "Subtract"
   },
   "position": [
    760,
    300
   ]
  },
  {
   "id": 21,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.5
   },
   "position": [
    950,
    300
   ]
  },
  {
   "id": 22,
   "node_type": {
    "type": "Subtract"
   },
   "position": [
    1140,
    300
   ]
  },
  {
   "id": 23,
   "node_type": {
    "type": "FloatConstant",
    "value": 1.0
   },
   "position": [
    1330,
    300
   ]
  },
  {
   "id": 24,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.12
   },
   "position": [
    0,
    450
   ]
  },
  {
   "id": 25,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.5
   },
   "position": [
    190,
    450
   ]
  },
  {
   "id": 26,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.5
   },
   "position": [
    380,
    450
   ]
  },
  {
   "id": 27,
   "node_type": {
    "type": "Subtract"
   },
   "position": [
    570,
    450
   ]
  },
  {
   "id": 28,
   "node_type": {
    "type": "Abs"
   },
   "position": [
    760,
    450
   ]
  },
  {
   "id": 29,
   "node_type": {
    "type": "SmoothStep"
   },
   "position": [
    950,
    450
   ]
  },
  {
   "id": 30,
   "node_type": {
    "type": "Subtract"
   },
   "position": [
    1140,
    450
   ]
  },
  {
   "id": 31,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.0
   },
   "position": [
    1330,
    450
   ]
  },
  {
   "id": 32,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.75
   },
   "position": [
    0,
    600
   ]
  },
  {
   "id": 33,
   "node_type": {
    "type": "SmoothStep"
   },
   "position": [
    190,
    600
   ]
  },
  {
   "id": 34,
   "node_type": {
    "type": "FloatConstant",
    "value": 1.0
   },
   "position": [
    380,
    600
   ]
  },
  {
   "id": 35,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.88
   },
   "position": [
    570,
    600
   ]
  },
  {
   "id": 36,
   "node_type": {
    "type": "FloatConstant",
    "value": 1.0
   },
   "position": [
    760,
    600
   ]
  },
  {
   "id": 37,
   "node_type": {
    "type": "SmoothStep"
   },
   "position": [
    950,
    600
   ]
  },
  {
   "id": 38,
   "node_type": {
    "type": "Subtract"
   },
   "position": [
    1140,
    600
   ]
  },
  {
   "id": 39,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1330,
    600
   ]
  },
  {
   "id": 40,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.016
   },
   "position": [
    0,
    750
   ]
  },
  {
   "id": 41,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    190,
    750
   ]
  },
  {
   "id": 42,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    380,
    750
   ]
  },
  {
   "id": 43,
   "node_type": {
    "type": "Combine",
    "components": 2
   },
   "position": [
    570,
    750
   ]
  },
  {
   "id": 44,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    760,
    750
   ]
  },
  {
   "id": 45,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.5
   },
   "position": [
    950,
    750
   ]
  },
  {
   "id": 46,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1140,
    750
   ]
  },
  {
   "id": 47,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1330,
    750
   ]
  },
  {
   "id": 48,
   "node_type": {
    "type": "SceneColor"
   },
   "position": [
    0,
    900
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
   "from_node": 1,
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
   "from_node": 4,
   "from_pin": 0,
   "to_node": 8,
   "to_pin": 0
  },
  {
   "from_node": 7,
   "from_pin": 0,
   "to_node": 8,
   "to_pin": 1
  },
  {
   "from_node": 8,
   "from_pin": 0,
   "to_node": 9,
   "to_pin": 0
  },
  {
   "from_node": 10,
   "from_pin": 0,
   "to_node": 9,
   "to_pin": 1
  },
  {
   "from_node": 4,
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
   "from_node": 13,
   "from_pin": 0,
   "to_node": 15,
   "to_pin": 0
  },
  {
   "from_node": 14,
   "from_pin": 0,
   "to_node": 15,
   "to_pin": 1
  },
  {
   "from_node": 12,
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
   "from_node": 18,
   "from_pin": 0,
   "to_node": 17,
   "to_pin": 1
  },
  {
   "from_node": 9,
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
   "from_node": 17,
   "from_pin": 0,
   "to_node": 22,
   "to_pin": 0
  },
  {
   "from_node": 21,
   "from_pin": 0,
   "to_node": 22,
   "to_pin": 1
  },
  {
   "from_node": 2,
   "from_pin": 0,
   "to_node": 27,
   "to_pin": 0
  },
  {
   "from_node": 26,
   "from_pin": 0,
   "to_node": 27,
   "to_pin": 1
  },
  {
   "from_node": 27,
   "from_pin": 0,
   "to_node": 28,
   "to_pin": 0
  },
  {
   "from_node": 24,
   "from_pin": 0,
   "to_node": 29,
   "to_pin": 0
  },
  {
   "from_node": 25,
   "from_pin": 0,
   "to_node": 29,
   "to_pin": 1
  },
  {
   "from_node": 28,
   "from_pin": 0,
   "to_node": 29,
   "to_pin": 2
  },
  {
   "from_node": 23,
   "from_pin": 0,
   "to_node": 30,
   "to_pin": 0
  },
  {
   "from_node": 29,
   "from_pin": 0,
   "to_node": 30,
   "to_pin": 1
  },
  {
   "from_node": 31,
   "from_pin": 0,
   "to_node": 33,
   "to_pin": 0
  },
  {
   "from_node": 32,
   "from_pin": 0,
   "to_node": 33,
   "to_pin": 1
  },
  {
   "from_node": 2,
   "from_pin": 1,
   "to_node": 33,
   "to_pin": 2
  },
  {
   "from_node": 35,
   "from_pin": 0,
   "to_node": 37,
   "to_pin": 0
  },
  {
   "from_node": 36,
   "from_pin": 0,
   "to_node": 37,
   "to_pin": 1
  },
  {
   "from_node": 2,
   "from_pin": 1,
   "to_node": 37,
   "to_pin": 2
  },
  {
   "from_node": 34,
   "from_pin": 0,
   "to_node": 38,
   "to_pin": 0
  },
  {
   "from_node": 37,
   "from_pin": 0,
   "to_node": 38,
   "to_pin": 1
  },
  {
   "from_node": 30,
   "from_pin": 0,
   "to_node": 39,
   "to_pin": 0
  },
  {
   "from_node": 33,
   "from_pin": 0,
   "to_node": 39,
   "to_pin": 1
  },
  {
   "from_node": 38,
   "from_pin": 0,
   "to_node": 41,
   "to_pin": 0
  },
  {
   "from_node": 40,
   "from_pin": 0,
   "to_node": 41,
   "to_pin": 1
  },
  {
   "from_node": 39,
   "from_pin": 0,
   "to_node": 42,
   "to_pin": 0
  },
  {
   "from_node": 41,
   "from_pin": 0,
   "to_node": 42,
   "to_pin": 1
  },
  {
   "from_node": 20,
   "from_pin": 0,
   "to_node": 44,
   "to_pin": 0
  },
  {
   "from_node": 42,
   "from_pin": 0,
   "to_node": 44,
   "to_pin": 1
  },
  {
   "from_node": 44,
   "from_pin": 0,
   "to_node": 43,
   "to_pin": 0
  },
  {
   "from_node": 42,
   "from_pin": 0,
   "to_node": 46,
   "to_pin": 0
  },
  {
   "from_node": 45,
   "from_pin": 0,
   "to_node": 46,
   "to_pin": 1
  },
  {
   "from_node": 22,
   "from_pin": 0,
   "to_node": 47,
   "to_pin": 0
  },
  {
   "from_node": 46,
   "from_pin": 0,
   "to_node": 47,
   "to_pin": 1
  },
  {
   "from_node": 47,
   "from_pin": 0,
   "to_node": 43,
   "to_pin": 1
  },
  {
   "from_node": 43,
   "from_pin": 0,
   "to_node": 48,
   "to_pin": 0
  },
  {
   "from_node": 48,
   "from_pin": 0,
   "to_node": 0,
   "to_pin": 0
  }
 ],
 "next_id": 49
}
