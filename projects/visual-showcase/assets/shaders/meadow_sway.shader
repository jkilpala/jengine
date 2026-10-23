{
 "meta": {
  "name": "Meadow sway",
  "description": "Grass and flowers in the wind, by moving their vertices (a VertexOffset node): each top leans with a breeze whose phase runs along the field, and gusts roll across it left to right - a band of stronger wind travelling through the world, so the grass bends in a wave. The foot stays (times 1 - uv.y) and a bent tip dips. `wind` is (how far a tip leans, in world units, at full strength, -, -, -).",
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
    "type": "VertexColorInput"
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
    "type": "PositionInput"
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
    "type": "FloatConstant",
    "value": 1.7
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
    "type": "FloatConstant",
    "value": 0.021
   },
   "position": [
    380,
    150
   ]
  },
  {
   "id": 11,
   "node_type": {
    "type": "Multiply"
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
    "type": "Sin"
   },
   "position": [
    950,
    150
   ]
  },
  {
   "id": 14,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.35
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
    "type": "FloatConstant",
    "value": 2.9
   },
   "position": [
    0,
    300
   ]
  },
  {
   "id": 17,
   "node_type": {
    "type": "Multiply"
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
    "value": 0.057
   },
   "position": [
    380,
    300
   ]
  },
  {
   "id": 19,
   "node_type": {
    "type": "Multiply"
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
    "type": "Sin"
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
    "value": 0.15
   },
   "position": [
    1140,
    300
   ]
  },
  {
   "id": 23,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1330,
    300
   ]
  },
  {
   "id": 24,
   "node_type": {
    "type": "Add"
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
    "value": 0.0035
   },
   "position": [
    190,
    450
   ]
  },
  {
   "id": 26,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    380,
    450
   ]
  },
  {
   "id": 27,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.9
   },
   "position": [
    570,
    450
   ]
  },
  {
   "id": 28,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    760,
    450
   ]
  },
  {
   "id": 29,
   "node_type": {
    "type": "Subtract"
   },
   "position": [
    950,
    450
   ]
  },
  {
   "id": 30,
   "node_type": {
    "type": "Sin"
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
    "type": "Max"
   },
   "position": [
    0,
    600
   ]
  },
  {
   "id": 33,
   "node_type": {
    "type": "Multiply"
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
    "value": 1.4
   },
   "position": [
    380,
    600
   ]
  },
  {
   "id": 35,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    570,
    600
   ]
  },
  {
   "id": 36,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    760,
    600
   ]
  },
  {
   "id": 37,
   "node_type": {
    "type": "MaterialParam",
    "name": "wind"
   },
   "position": [
    950,
    600
   ]
  },
  {
   "id": 38,
   "node_type": {
    "type": "Split"
   },
   "position": [
    1140,
    600
   ]
  },
  {
   "id": 39,
   "node_type": {
    "type": "FloatConstant",
    "value": 1.0
   },
   "position": [
    1330,
    600
   ]
  },
  {
   "id": 40,
   "node_type": {
    "type": "Split"
   },
   "position": [
    0,
    750
   ]
  },
  {
   "id": 41,
   "node_type": {
    "type": "Subtract"
   },
   "position": [
    190,
    750
   ]
  },
  {
   "id": 42,
   "node_type": {
    "type": "Add"
   },
   "position": [
    380,
    750
   ]
  },
  {
   "id": 43,
   "node_type": {
    "type": "Multiply"
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
    "type": "Abs"
   },
   "position": [
    950,
    750
   ]
  },
  {
   "id": 46,
   "node_type": {
    "type": "FloatConstant",
    "value": -0.3
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
    "type": "Combine",
    "components": 2
   },
   "position": [
    0,
    900
   ]
  },
  {
   "id": 49,
   "node_type": {
    "type": "VertexOffset"
   },
   "position": [
    190,
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
   "from_node": 6,
   "from_pin": 0,
   "to_node": 7,
   "to_pin": 0
  },
  {
   "from_node": 5,
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
   "from_node": 7,
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
   "from_node": 9,
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
   "from_node": 5,
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
   "from_node": 7,
   "from_pin": 0,
   "to_node": 19,
   "to_pin": 0
  },
  {
   "from_node": 18,
   "from_pin": 0,
   "to_node": 19,
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
   "to_node": 24,
   "to_pin": 0
  },
  {
   "from_node": 23,
   "from_pin": 0,
   "to_node": 24,
   "to_pin": 1
  },
  {
   "from_node": 7,
   "from_pin": 0,
   "to_node": 26,
   "to_pin": 0
  },
  {
   "from_node": 25,
   "from_pin": 0,
   "to_node": 26,
   "to_pin": 1
  },
  {
   "from_node": 5,
   "from_pin": 0,
   "to_node": 28,
   "to_pin": 0
  },
  {
   "from_node": 27,
   "from_pin": 0,
   "to_node": 28,
   "to_pin": 1
  },
  {
   "from_node": 26,
   "from_pin": 0,
   "to_node": 29,
   "to_pin": 0
  },
  {
   "from_node": 28,
   "from_pin": 0,
   "to_node": 29,
   "to_pin": 1
  },
  {
   "from_node": 29,
   "from_pin": 0,
   "to_node": 30,
   "to_pin": 0
  },
  {
   "from_node": 30,
   "from_pin": 0,
   "to_node": 32,
   "to_pin": 0
  },
  {
   "from_node": 31,
   "from_pin": 0,
   "to_node": 32,
   "to_pin": 1
  },
  {
   "from_node": 32,
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
   "from_node": 32,
   "from_pin": 0,
   "to_node": 35,
   "to_pin": 0
  },
  {
   "from_node": 34,
   "from_pin": 0,
   "to_node": 35,
   "to_pin": 1
  },
  {
   "from_node": 33,
   "from_pin": 0,
   "to_node": 36,
   "to_pin": 0
  },
  {
   "from_node": 35,
   "from_pin": 0,
   "to_node": 36,
   "to_pin": 1
  },
  {
   "from_node": 37,
   "from_pin": 0,
   "to_node": 38,
   "to_pin": 0
  },
  {
   "from_node": 1,
   "from_pin": 0,
   "to_node": 40,
   "to_pin": 0
  },
  {
   "from_node": 39,
   "from_pin": 0,
   "to_node": 41,
   "to_pin": 0
  },
  {
   "from_node": 40,
   "from_pin": 1,
   "to_node": 41,
   "to_pin": 1
  },
  {
   "from_node": 24,
   "from_pin": 0,
   "to_node": 42,
   "to_pin": 0
  },
  {
   "from_node": 36,
   "from_pin": 0,
   "to_node": 42,
   "to_pin": 1
  },
  {
   "from_node": 42,
   "from_pin": 0,
   "to_node": 43,
   "to_pin": 0
  },
  {
   "from_node": 38,
   "from_pin": 0,
   "to_node": 43,
   "to_pin": 1
  },
  {
   "from_node": 43,
   "from_pin": 0,
   "to_node": 44,
   "to_pin": 0
  },
  {
   "from_node": 41,
   "from_pin": 0,
   "to_node": 44,
   "to_pin": 1
  },
  {
   "from_node": 44,
   "from_pin": 0,
   "to_node": 45,
   "to_pin": 0
  },
  {
   "from_node": 45,
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
   "from_node": 44,
   "from_pin": 0,
   "to_node": 48,
   "to_pin": 0
  },
  {
   "from_node": 47,
   "from_pin": 0,
   "to_node": 48,
   "to_pin": 1
  },
  {
   "from_node": 48,
   "from_pin": 0,
   "to_node": 49,
   "to_pin": 0
  }
 ],
 "next_id": 50
}
