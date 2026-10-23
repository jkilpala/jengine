{
 "meta": {
  "name": "Lava",
  "description": "Noise warped by noise and pushed along slowly, through a colour ramp from cooling crust to a white-hot vein brighter than white.",
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
    "type": "TimeInput"
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
     0.01,
     0.03
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
    "type": "Add"
   },
   "position": [
    950,
    0
   ]
  },
  {
   "id": 6,
   "node_type": {
    "type": "Noise",
    "octaves": 4
   },
   "position": [
    1140,
    0
   ]
  },
  {
   "id": 7,
   "node_type": {
    "type": "FloatConstant",
    "value": 3.0
   },
   "position": [
    1330,
    0
   ]
  },
  {
   "id": 8,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.45
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
    "type": "Add"
   },
   "position": [
    380,
    150
   ]
  },
  {
   "id": 11,
   "node_type": {
    "type": "TimeInput"
   },
   "position": [
    570,
    150
   ]
  },
  {
   "id": 12,
   "node_type": {
    "type": "Vec2Constant",
    "value": [
     -0.02,
     0.05
    ]
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
    "type": "Add"
   },
   "position": [
    1140,
    150
   ]
  },
  {
   "id": 15,
   "node_type": {
    "type": "Noise",
    "octaves": 5
   },
   "position": [
    1330,
    150
   ]
  },
  {
   "id": 16,
   "node_type": {
    "type": "FloatConstant",
    "value": 4.0
   },
   "position": [
    0,
    300
   ]
  },
  {
   "id": 17,
   "node_type": {
    "type": "ColorInput",
    "color": [
     0.05,
     0.01,
     0.01,
     1.0
    ]
   },
   "position": [
    190,
    300
   ]
  },
  {
   "id": 18,
   "node_type": {
    "type": "ColorInput",
    "color": [
     0.9,
     0.12,
     0.02,
     1.0
    ]
   },
   "position": [
    380,
    300
   ]
  },
  {
   "id": 19,
   "node_type": {
    "type": "ColorInput",
    "color": [
     2.4,
     0.9,
     0.12,
     1.0
    ]
   },
   "position": [
    570,
    300
   ]
  },
  {
   "id": 20,
   "node_type": {
    "type": "ColorInput",
    "color": [
     5.0,
     3.4,
     1.4,
     1.0
    ]
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
    "value": 0.38
   },
   "position": [
    950,
    300
   ]
  },
  {
   "id": 22,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.55
   },
   "position": [
    1140,
    300
   ]
  },
  {
   "id": 23,
   "node_type": {
    "type": "SmoothStep"
   },
   "position": [
    1330,
    300
   ]
  },
  {
   "id": 24,
   "node_type": {
    "type": "Mix"
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
    "value": 0.55
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
    "value": 0.68
   },
   "position": [
    380,
    450
   ]
  },
  {
   "id": 27,
   "node_type": {
    "type": "SmoothStep"
   },
   "position": [
    570,
    450
   ]
  },
  {
   "id": 28,
   "node_type": {
    "type": "Mix"
   },
   "position": [
    760,
    450
   ]
  },
  {
   "id": 29,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.72
   },
   "position": [
    950,
    450
   ]
  },
  {
   "id": 30,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.82
   },
   "position": [
    1140,
    450
   ]
  },
  {
   "id": 31,
   "node_type": {
    "type": "SmoothStep"
   },
   "position": [
    1330,
    450
   ]
  },
  {
   "id": 32,
   "node_type": {
    "type": "Mix"
   },
   "position": [
    0,
    600
   ]
  }
 ],
 "edges": [
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
   "from_node": 1,
   "from_pin": 0,
   "to_node": 5,
   "to_pin": 0
  },
  {
   "from_node": 4,
   "from_pin": 0,
   "to_node": 5,
   "to_pin": 1
  },
  {
   "from_node": 5,
   "from_pin": 0,
   "to_node": 6,
   "to_pin": 0
  },
  {
   "from_node": 7,
   "from_pin": 0,
   "to_node": 6,
   "to_pin": 1
  },
  {
   "from_node": 6,
   "from_pin": 0,
   "to_node": 9,
   "to_pin": 0
  },
  {
   "from_node": 8,
   "from_pin": 0,
   "to_node": 9,
   "to_pin": 1
  },
  {
   "from_node": 1,
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
   "from_node": 10,
   "from_pin": 0,
   "to_node": 14,
   "to_pin": 0
  },
  {
   "from_node": 13,
   "from_pin": 0,
   "to_node": 14,
   "to_pin": 1
  },
  {
   "from_node": 14,
   "from_pin": 0,
   "to_node": 15,
   "to_pin": 0
  },
  {
   "from_node": 16,
   "from_pin": 0,
   "to_node": 15,
   "to_pin": 1
  },
  {
   "from_node": 21,
   "from_pin": 0,
   "to_node": 23,
   "to_pin": 0
  },
  {
   "from_node": 22,
   "from_pin": 0,
   "to_node": 23,
   "to_pin": 1
  },
  {
   "from_node": 15,
   "from_pin": 0,
   "to_node": 23,
   "to_pin": 2
  },
  {
   "from_node": 17,
   "from_pin": 0,
   "to_node": 24,
   "to_pin": 0
  },
  {
   "from_node": 18,
   "from_pin": 0,
   "to_node": 24,
   "to_pin": 1
  },
  {
   "from_node": 23,
   "from_pin": 0,
   "to_node": 24,
   "to_pin": 2
  },
  {
   "from_node": 25,
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
   "from_node": 15,
   "from_pin": 0,
   "to_node": 27,
   "to_pin": 2
  },
  {
   "from_node": 24,
   "from_pin": 0,
   "to_node": 28,
   "to_pin": 0
  },
  {
   "from_node": 19,
   "from_pin": 0,
   "to_node": 28,
   "to_pin": 1
  },
  {
   "from_node": 27,
   "from_pin": 0,
   "to_node": 28,
   "to_pin": 2
  },
  {
   "from_node": 29,
   "from_pin": 0,
   "to_node": 31,
   "to_pin": 0
  },
  {
   "from_node": 30,
   "from_pin": 0,
   "to_node": 31,
   "to_pin": 1
  },
  {
   "from_node": 15,
   "from_pin": 0,
   "to_node": 31,
   "to_pin": 2
  },
  {
   "from_node": 28,
   "from_pin": 0,
   "to_node": 32,
   "to_pin": 0
  },
  {
   "from_node": 20,
   "from_pin": 0,
   "to_node": 32,
   "to_pin": 1
  },
  {
   "from_node": 31,
   "from_pin": 0,
   "to_node": 32,
   "to_pin": 2
  },
  {
   "from_node": 32,
   "from_pin": 0,
   "to_node": 0,
   "to_pin": 0
  }
 ],
 "next_id": 33
}
