{
 "meta": {
  "name": "Ghost glow",
  "description": "For a canvas group: the group's whole picture rippling a little, with a glow round its silhouette - round the group, not round each of its parts, which overlap inside it. The picture is premultiplied, so this is too (the material blends PremultipliedAlpha). `glow` is (width in texels, strength, ripple, -); `glow_color` its colour.",
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
    "type": "MaterialParam",
    "name": "glow"
   },
   "position": [
    570,
    0
   ]
  },
  {
   "id": 4,
   "node_type": {
    "type": "Split"
   },
   "position": [
    760,
    0
   ]
  },
  {
   "id": 5,
   "node_type": {
    "type": "Split"
   },
   "position": [
    950,
    0
   ]
  },
  {
   "id": 6,
   "node_type": {
    "type": "FloatConstant",
    "value": 46.0
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
    "type": "FloatConstant",
    "value": 2.6
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
    "type": "Sin"
   },
   "position": [
    570,
    150
   ]
  },
  {
   "id": 12,
   "node_type": {
    "type": "Combine",
    "components": 2
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
    "type": "FloatConstant",
    "value": 0.0
   },
   "position": [
    1140,
    150
   ]
  },
  {
   "id": 15,
   "node_type": {
    "type": "Add"
   },
   "position": [
    1330,
    150
   ]
  },
  {
   "id": 16,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    0,
    300
   ]
  },
  {
   "id": 17,
   "node_type": {
    "type": "VertexColorInput"
   },
   "position": [
    190,
    300
   ]
  },
  {
   "id": 18,
   "node_type": {
    "type": "Split"
   },
   "position": [
    380,
    300
   ]
  },
  {
   "id": 19,
   "node_type": {
    "type": "TexelSize"
   },
   "position": [
    570,
    300
   ]
  },
  {
   "id": 20,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    760,
    300
   ]
  },
  {
   "id": 21,
   "node_type": {
    "type": "Split"
   },
   "position": [
    950,
    300
   ]
  },
  {
   "id": 22,
   "node_type": {
    "type": "Combine",
    "components": 2
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
    "type": "Multiply"
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
    "value": 0.0
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
    "type": "Add"
   },
   "position": [
    570,
    450
   ]
  },
  {
   "id": 28,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    760,
    450
   ]
  },
  {
   "id": 29,
   "node_type": {
    "type": "Combine",
    "components": 2
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
    "value": -1.0
   },
   "position": [
    1140,
    450
   ]
  },
  {
   "id": 31,
   "node_type": {
    "type": "Multiply"
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
    "value": 0.0
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
    "type": "Add"
   },
   "position": [
    380,
    600
   ]
  },
  {
   "id": 35,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    570,
    600
   ]
  },
  {
   "id": 36,
   "node_type": {
    "type": "Max"
   },
   "position": [
    760,
    600
   ]
  },
  {
   "id": 37,
   "node_type": {
    "type": "Combine",
    "components": 2
   },
   "position": [
    950,
    600
   ]
  },
  {
   "id": 38,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.0
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
    "value": 1.0
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
    "type": "TextureSample"
   },
   "position": [
    570,
    750
   ]
  },
  {
   "id": 44,
   "node_type": {
    "type": "Max"
   },
   "position": [
    760,
    750
   ]
  },
  {
   "id": 45,
   "node_type": {
    "type": "Combine",
    "components": 2
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
    "value": 0.0
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
    "type": "FloatConstant",
    "value": -1.0
   },
   "position": [
    0,
    900
   ]
  },
  {
   "id": 49,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    190,
    900
   ]
  },
  {
   "id": 50,
   "node_type": {
    "type": "Add"
   },
   "position": [
    380,
    900
   ]
  },
  {
   "id": 51,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    570,
    900
   ]
  },
  {
   "id": 52,
   "node_type": {
    "type": "Max"
   },
   "position": [
    760,
    900
   ]
  },
  {
   "id": 53,
   "node_type": {
    "type": "Combine",
    "components": 2
   },
   "position": [
    950,
    900
   ]
  },
  {
   "id": 54,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.7
   },
   "position": [
    1140,
    900
   ]
  },
  {
   "id": 55,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1330,
    900
   ]
  },
  {
   "id": 56,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.7
   },
   "position": [
    0,
    1050
   ]
  },
  {
   "id": 57,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    190,
    1050
   ]
  },
  {
   "id": 58,
   "node_type": {
    "type": "Add"
   },
   "position": [
    380,
    1050
   ]
  },
  {
   "id": 59,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    570,
    1050
   ]
  },
  {
   "id": 60,
   "node_type": {
    "type": "Max"
   },
   "position": [
    760,
    1050
   ]
  },
  {
   "id": 61,
   "node_type": {
    "type": "Combine",
    "components": 2
   },
   "position": [
    950,
    1050
   ]
  },
  {
   "id": 62,
   "node_type": {
    "type": "FloatConstant",
    "value": -0.7
   },
   "position": [
    1140,
    1050
   ]
  },
  {
   "id": 63,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1330,
    1050
   ]
  },
  {
   "id": 64,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.7
   },
   "position": [
    0,
    1200
   ]
  },
  {
   "id": 65,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    190,
    1200
   ]
  },
  {
   "id": 66,
   "node_type": {
    "type": "Add"
   },
   "position": [
    380,
    1200
   ]
  },
  {
   "id": 67,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    570,
    1200
   ]
  },
  {
   "id": 68,
   "node_type": {
    "type": "Max"
   },
   "position": [
    760,
    1200
   ]
  },
  {
   "id": 69,
   "node_type": {
    "type": "Combine",
    "components": 2
   },
   "position": [
    950,
    1200
   ]
  },
  {
   "id": 70,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.7
   },
   "position": [
    1140,
    1200
   ]
  },
  {
   "id": 71,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1330,
    1200
   ]
  },
  {
   "id": 72,
   "node_type": {
    "type": "FloatConstant",
    "value": -0.7
   },
   "position": [
    0,
    1350
   ]
  },
  {
   "id": 73,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    190,
    1350
   ]
  },
  {
   "id": 74,
   "node_type": {
    "type": "Add"
   },
   "position": [
    380,
    1350
   ]
  },
  {
   "id": 75,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    570,
    1350
   ]
  },
  {
   "id": 76,
   "node_type": {
    "type": "Max"
   },
   "position": [
    760,
    1350
   ]
  },
  {
   "id": 77,
   "node_type": {
    "type": "Combine",
    "components": 2
   },
   "position": [
    950,
    1350
   ]
  },
  {
   "id": 78,
   "node_type": {
    "type": "FloatConstant",
    "value": -0.7
   },
   "position": [
    1140,
    1350
   ]
  },
  {
   "id": 79,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1330,
    1350
   ]
  },
  {
   "id": 80,
   "node_type": {
    "type": "FloatConstant",
    "value": -0.7
   },
   "position": [
    0,
    1500
   ]
  },
  {
   "id": 81,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    190,
    1500
   ]
  },
  {
   "id": 82,
   "node_type": {
    "type": "Add"
   },
   "position": [
    380,
    1500
   ]
  },
  {
   "id": 83,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    570,
    1500
   ]
  },
  {
   "id": 84,
   "node_type": {
    "type": "Max"
   },
   "position": [
    760,
    1500
   ]
  },
  {
   "id": 85,
   "node_type": {
    "type": "Combine",
    "components": 2
   },
   "position": [
    950,
    1500
   ]
  },
  {
   "id": 86,
   "node_type": {
    "type": "FloatConstant",
    "value": 2.0
   },
   "position": [
    1140,
    1500
   ]
  },
  {
   "id": 87,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1330,
    1500
   ]
  },
  {
   "id": 88,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.0
   },
   "position": [
    0,
    1650
   ]
  },
  {
   "id": 89,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    190,
    1650
   ]
  },
  {
   "id": 90,
   "node_type": {
    "type": "Add"
   },
   "position": [
    380,
    1650
   ]
  },
  {
   "id": 91,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    570,
    1650
   ]
  },
  {
   "id": 92,
   "node_type": {
    "type": "Max"
   },
   "position": [
    760,
    1650
   ]
  },
  {
   "id": 93,
   "node_type": {
    "type": "Combine",
    "components": 2
   },
   "position": [
    950,
    1650
   ]
  },
  {
   "id": 94,
   "node_type": {
    "type": "FloatConstant",
    "value": -2.0
   },
   "position": [
    1140,
    1650
   ]
  },
  {
   "id": 95,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1330,
    1650
   ]
  },
  {
   "id": 96,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.0
   },
   "position": [
    0,
    1800
   ]
  },
  {
   "id": 97,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    190,
    1800
   ]
  },
  {
   "id": 98,
   "node_type": {
    "type": "Add"
   },
   "position": [
    380,
    1800
   ]
  },
  {
   "id": 99,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    570,
    1800
   ]
  },
  {
   "id": 100,
   "node_type": {
    "type": "Max"
   },
   "position": [
    760,
    1800
   ]
  },
  {
   "id": 101,
   "node_type": {
    "type": "Combine",
    "components": 2
   },
   "position": [
    950,
    1800
   ]
  },
  {
   "id": 102,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.0
   },
   "position": [
    1140,
    1800
   ]
  },
  {
   "id": 103,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1330,
    1800
   ]
  },
  {
   "id": 104,
   "node_type": {
    "type": "FloatConstant",
    "value": 2.0
   },
   "position": [
    0,
    1950
   ]
  },
  {
   "id": 105,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    190,
    1950
   ]
  },
  {
   "id": 106,
   "node_type": {
    "type": "Add"
   },
   "position": [
    380,
    1950
   ]
  },
  {
   "id": 107,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    570,
    1950
   ]
  },
  {
   "id": 108,
   "node_type": {
    "type": "Max"
   },
   "position": [
    760,
    1950
   ]
  },
  {
   "id": 109,
   "node_type": {
    "type": "Combine",
    "components": 2
   },
   "position": [
    950,
    1950
   ]
  },
  {
   "id": 110,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.0
   },
   "position": [
    1140,
    1950
   ]
  },
  {
   "id": 111,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1330,
    1950
   ]
  },
  {
   "id": 112,
   "node_type": {
    "type": "FloatConstant",
    "value": -2.0
   },
   "position": [
    0,
    2100
   ]
  },
  {
   "id": 113,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    190,
    2100
   ]
  },
  {
   "id": 114,
   "node_type": {
    "type": "Add"
   },
   "position": [
    380,
    2100
   ]
  },
  {
   "id": 115,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    570,
    2100
   ]
  },
  {
   "id": 116,
   "node_type": {
    "type": "Max"
   },
   "position": [
    760,
    2100
   ]
  },
  {
   "id": 117,
   "node_type": {
    "type": "Subtract"
   },
   "position": [
    950,
    2100
   ]
  },
  {
   "id": 118,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.0
   },
   "position": [
    1140,
    2100
   ]
  },
  {
   "id": 119,
   "node_type": {
    "type": "Max"
   },
   "position": [
    1330,
    2100
   ]
  },
  {
   "id": 120,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    0,
    2250
   ]
  },
  {
   "id": 121,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    190,
    2250
   ]
  },
  {
   "id": 122,
   "node_type": {
    "type": "MaterialParam",
    "name": "glow_color"
   },
   "position": [
    380,
    2250
   ]
  },
  {
   "id": 123,
   "node_type": {
    "type": "Split"
   },
   "position": [
    570,
    2250
   ]
  },
  {
   "id": 124,
   "node_type": {
    "type": "FloatConstant",
    "value": 1.0
   },
   "position": [
    760,
    2250
   ]
  },
  {
   "id": 125,
   "node_type": {
    "type": "Subtract"
   },
   "position": [
    950,
    2250
   ]
  },
  {
   "id": 126,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1140,
    2250
   ]
  },
  {
   "id": 127,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1330,
    2250
   ]
  },
  {
   "id": 128,
   "node_type": {
    "type": "Split"
   },
   "position": [
    0,
    2400
   ]
  },
  {
   "id": 129,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    190,
    2400
   ]
  },
  {
   "id": 130,
   "node_type": {
    "type": "Add"
   },
   "position": [
    380,
    2400
   ]
  },
  {
   "id": 131,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    570,
    2400
   ]
  },
  {
   "id": 132,
   "node_type": {
    "type": "Add"
   },
   "position": [
    760,
    2400
   ]
  },
  {
   "id": 133,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    950,
    2400
   ]
  },
  {
   "id": 134,
   "node_type": {
    "type": "Add"
   },
   "position": [
    1140,
    2400
   ]
  },
  {
   "id": 135,
   "node_type": {
    "type": "Add"
   },
   "position": [
    1330,
    2400
   ]
  },
  {
   "id": 136,
   "node_type": {
    "type": "Combine",
    "components": 4
   },
   "position": [
    0,
    2550
   ]
  }
 ],
 "edges": [
  {
   "from_node": 3,
   "from_pin": 0,
   "to_node": 4,
   "to_pin": 0
  },
  {
   "from_node": 1,
   "from_pin": 0,
   "to_node": 5,
   "to_pin": 0
  },
  {
   "from_node": 5,
   "from_pin": 1,
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
   "from_node": 2,
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
   "from_node": 10,
   "from_pin": 0,
   "to_node": 11,
   "to_pin": 0
  },
  {
   "from_node": 11,
   "from_pin": 0,
   "to_node": 13,
   "to_pin": 0
  },
  {
   "from_node": 4,
   "from_pin": 2,
   "to_node": 13,
   "to_pin": 1
  },
  {
   "from_node": 13,
   "from_pin": 0,
   "to_node": 12,
   "to_pin": 0
  },
  {
   "from_node": 14,
   "from_pin": 0,
   "to_node": 12,
   "to_pin": 1
  },
  {
   "from_node": 1,
   "from_pin": 0,
   "to_node": 15,
   "to_pin": 0
  },
  {
   "from_node": 12,
   "from_pin": 0,
   "to_node": 15,
   "to_pin": 1
  },
  {
   "from_node": 15,
   "from_pin": 0,
   "to_node": 16,
   "to_pin": 0
  },
  {
   "from_node": 17,
   "from_pin": 0,
   "to_node": 18,
   "to_pin": 0
  },
  {
   "from_node": 19,
   "from_pin": 0,
   "to_node": 20,
   "to_pin": 0
  },
  {
   "from_node": 4,
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
   "from_node": 24,
   "from_pin": 0,
   "to_node": 22,
   "to_pin": 0
  },
  {
   "from_node": 21,
   "from_pin": 1,
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
   "from_node": 26,
   "from_pin": 0,
   "to_node": 22,
   "to_pin": 1
  },
  {
   "from_node": 15,
   "from_pin": 0,
   "to_node": 27,
   "to_pin": 0
  },
  {
   "from_node": 22,
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
   "from_node": 21,
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
   "from_node": 31,
   "from_pin": 0,
   "to_node": 29,
   "to_pin": 0
  },
  {
   "from_node": 21,
   "from_pin": 1,
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
   "from_node": 33,
   "from_pin": 0,
   "to_node": 29,
   "to_pin": 1
  },
  {
   "from_node": 15,
   "from_pin": 0,
   "to_node": 34,
   "to_pin": 0
  },
  {
   "from_node": 29,
   "from_pin": 0,
   "to_node": 34,
   "to_pin": 1
  },
  {
   "from_node": 34,
   "from_pin": 0,
   "to_node": 35,
   "to_pin": 0
  },
  {
   "from_node": 28,
   "from_pin": 4,
   "to_node": 36,
   "to_pin": 0
  },
  {
   "from_node": 35,
   "from_pin": 4,
   "to_node": 36,
   "to_pin": 1
  },
  {
   "from_node": 21,
   "from_pin": 0,
   "to_node": 39,
   "to_pin": 0
  },
  {
   "from_node": 38,
   "from_pin": 0,
   "to_node": 39,
   "to_pin": 1
  },
  {
   "from_node": 39,
   "from_pin": 0,
   "to_node": 37,
   "to_pin": 0
  },
  {
   "from_node": 21,
   "from_pin": 1,
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
   "from_node": 41,
   "from_pin": 0,
   "to_node": 37,
   "to_pin": 1
  },
  {
   "from_node": 15,
   "from_pin": 0,
   "to_node": 42,
   "to_pin": 0
  },
  {
   "from_node": 37,
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
   "from_node": 36,
   "from_pin": 0,
   "to_node": 44,
   "to_pin": 0
  },
  {
   "from_node": 43,
   "from_pin": 4,
   "to_node": 44,
   "to_pin": 1
  },
  {
   "from_node": 21,
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
   "to_node": 45,
   "to_pin": 0
  },
  {
   "from_node": 21,
   "from_pin": 1,
   "to_node": 49,
   "to_pin": 0
  },
  {
   "from_node": 48,
   "from_pin": 0,
   "to_node": 49,
   "to_pin": 1
  },
  {
   "from_node": 49,
   "from_pin": 0,
   "to_node": 45,
   "to_pin": 1
  },
  {
   "from_node": 15,
   "from_pin": 0,
   "to_node": 50,
   "to_pin": 0
  },
  {
   "from_node": 45,
   "from_pin": 0,
   "to_node": 50,
   "to_pin": 1
  },
  {
   "from_node": 50,
   "from_pin": 0,
   "to_node": 51,
   "to_pin": 0
  },
  {
   "from_node": 44,
   "from_pin": 0,
   "to_node": 52,
   "to_pin": 0
  },
  {
   "from_node": 51,
   "from_pin": 4,
   "to_node": 52,
   "to_pin": 1
  },
  {
   "from_node": 21,
   "from_pin": 0,
   "to_node": 55,
   "to_pin": 0
  },
  {
   "from_node": 54,
   "from_pin": 0,
   "to_node": 55,
   "to_pin": 1
  },
  {
   "from_node": 55,
   "from_pin": 0,
   "to_node": 53,
   "to_pin": 0
  },
  {
   "from_node": 21,
   "from_pin": 1,
   "to_node": 57,
   "to_pin": 0
  },
  {
   "from_node": 56,
   "from_pin": 0,
   "to_node": 57,
   "to_pin": 1
  },
  {
   "from_node": 57,
   "from_pin": 0,
   "to_node": 53,
   "to_pin": 1
  },
  {
   "from_node": 15,
   "from_pin": 0,
   "to_node": 58,
   "to_pin": 0
  },
  {
   "from_node": 53,
   "from_pin": 0,
   "to_node": 58,
   "to_pin": 1
  },
  {
   "from_node": 58,
   "from_pin": 0,
   "to_node": 59,
   "to_pin": 0
  },
  {
   "from_node": 52,
   "from_pin": 0,
   "to_node": 60,
   "to_pin": 0
  },
  {
   "from_node": 59,
   "from_pin": 4,
   "to_node": 60,
   "to_pin": 1
  },
  {
   "from_node": 21,
   "from_pin": 0,
   "to_node": 63,
   "to_pin": 0
  },
  {
   "from_node": 62,
   "from_pin": 0,
   "to_node": 63,
   "to_pin": 1
  },
  {
   "from_node": 63,
   "from_pin": 0,
   "to_node": 61,
   "to_pin": 0
  },
  {
   "from_node": 21,
   "from_pin": 1,
   "to_node": 65,
   "to_pin": 0
  },
  {
   "from_node": 64,
   "from_pin": 0,
   "to_node": 65,
   "to_pin": 1
  },
  {
   "from_node": 65,
   "from_pin": 0,
   "to_node": 61,
   "to_pin": 1
  },
  {
   "from_node": 15,
   "from_pin": 0,
   "to_node": 66,
   "to_pin": 0
  },
  {
   "from_node": 61,
   "from_pin": 0,
   "to_node": 66,
   "to_pin": 1
  },
  {
   "from_node": 66,
   "from_pin": 0,
   "to_node": 67,
   "to_pin": 0
  },
  {
   "from_node": 60,
   "from_pin": 0,
   "to_node": 68,
   "to_pin": 0
  },
  {
   "from_node": 67,
   "from_pin": 4,
   "to_node": 68,
   "to_pin": 1
  },
  {
   "from_node": 21,
   "from_pin": 0,
   "to_node": 71,
   "to_pin": 0
  },
  {
   "from_node": 70,
   "from_pin": 0,
   "to_node": 71,
   "to_pin": 1
  },
  {
   "from_node": 71,
   "from_pin": 0,
   "to_node": 69,
   "to_pin": 0
  },
  {
   "from_node": 21,
   "from_pin": 1,
   "to_node": 73,
   "to_pin": 0
  },
  {
   "from_node": 72,
   "from_pin": 0,
   "to_node": 73,
   "to_pin": 1
  },
  {
   "from_node": 73,
   "from_pin": 0,
   "to_node": 69,
   "to_pin": 1
  },
  {
   "from_node": 15,
   "from_pin": 0,
   "to_node": 74,
   "to_pin": 0
  },
  {
   "from_node": 69,
   "from_pin": 0,
   "to_node": 74,
   "to_pin": 1
  },
  {
   "from_node": 74,
   "from_pin": 0,
   "to_node": 75,
   "to_pin": 0
  },
  {
   "from_node": 68,
   "from_pin": 0,
   "to_node": 76,
   "to_pin": 0
  },
  {
   "from_node": 75,
   "from_pin": 4,
   "to_node": 76,
   "to_pin": 1
  },
  {
   "from_node": 21,
   "from_pin": 0,
   "to_node": 79,
   "to_pin": 0
  },
  {
   "from_node": 78,
   "from_pin": 0,
   "to_node": 79,
   "to_pin": 1
  },
  {
   "from_node": 79,
   "from_pin": 0,
   "to_node": 77,
   "to_pin": 0
  },
  {
   "from_node": 21,
   "from_pin": 1,
   "to_node": 81,
   "to_pin": 0
  },
  {
   "from_node": 80,
   "from_pin": 0,
   "to_node": 81,
   "to_pin": 1
  },
  {
   "from_node": 81,
   "from_pin": 0,
   "to_node": 77,
   "to_pin": 1
  },
  {
   "from_node": 15,
   "from_pin": 0,
   "to_node": 82,
   "to_pin": 0
  },
  {
   "from_node": 77,
   "from_pin": 0,
   "to_node": 82,
   "to_pin": 1
  },
  {
   "from_node": 82,
   "from_pin": 0,
   "to_node": 83,
   "to_pin": 0
  },
  {
   "from_node": 76,
   "from_pin": 0,
   "to_node": 84,
   "to_pin": 0
  },
  {
   "from_node": 83,
   "from_pin": 4,
   "to_node": 84,
   "to_pin": 1
  },
  {
   "from_node": 21,
   "from_pin": 0,
   "to_node": 87,
   "to_pin": 0
  },
  {
   "from_node": 86,
   "from_pin": 0,
   "to_node": 87,
   "to_pin": 1
  },
  {
   "from_node": 87,
   "from_pin": 0,
   "to_node": 85,
   "to_pin": 0
  },
  {
   "from_node": 21,
   "from_pin": 1,
   "to_node": 89,
   "to_pin": 0
  },
  {
   "from_node": 88,
   "from_pin": 0,
   "to_node": 89,
   "to_pin": 1
  },
  {
   "from_node": 89,
   "from_pin": 0,
   "to_node": 85,
   "to_pin": 1
  },
  {
   "from_node": 15,
   "from_pin": 0,
   "to_node": 90,
   "to_pin": 0
  },
  {
   "from_node": 85,
   "from_pin": 0,
   "to_node": 90,
   "to_pin": 1
  },
  {
   "from_node": 90,
   "from_pin": 0,
   "to_node": 91,
   "to_pin": 0
  },
  {
   "from_node": 84,
   "from_pin": 0,
   "to_node": 92,
   "to_pin": 0
  },
  {
   "from_node": 91,
   "from_pin": 4,
   "to_node": 92,
   "to_pin": 1
  },
  {
   "from_node": 21,
   "from_pin": 0,
   "to_node": 95,
   "to_pin": 0
  },
  {
   "from_node": 94,
   "from_pin": 0,
   "to_node": 95,
   "to_pin": 1
  },
  {
   "from_node": 95,
   "from_pin": 0,
   "to_node": 93,
   "to_pin": 0
  },
  {
   "from_node": 21,
   "from_pin": 1,
   "to_node": 97,
   "to_pin": 0
  },
  {
   "from_node": 96,
   "from_pin": 0,
   "to_node": 97,
   "to_pin": 1
  },
  {
   "from_node": 97,
   "from_pin": 0,
   "to_node": 93,
   "to_pin": 1
  },
  {
   "from_node": 15,
   "from_pin": 0,
   "to_node": 98,
   "to_pin": 0
  },
  {
   "from_node": 93,
   "from_pin": 0,
   "to_node": 98,
   "to_pin": 1
  },
  {
   "from_node": 98,
   "from_pin": 0,
   "to_node": 99,
   "to_pin": 0
  },
  {
   "from_node": 92,
   "from_pin": 0,
   "to_node": 100,
   "to_pin": 0
  },
  {
   "from_node": 99,
   "from_pin": 4,
   "to_node": 100,
   "to_pin": 1
  },
  {
   "from_node": 21,
   "from_pin": 0,
   "to_node": 103,
   "to_pin": 0
  },
  {
   "from_node": 102,
   "from_pin": 0,
   "to_node": 103,
   "to_pin": 1
  },
  {
   "from_node": 103,
   "from_pin": 0,
   "to_node": 101,
   "to_pin": 0
  },
  {
   "from_node": 21,
   "from_pin": 1,
   "to_node": 105,
   "to_pin": 0
  },
  {
   "from_node": 104,
   "from_pin": 0,
   "to_node": 105,
   "to_pin": 1
  },
  {
   "from_node": 105,
   "from_pin": 0,
   "to_node": 101,
   "to_pin": 1
  },
  {
   "from_node": 15,
   "from_pin": 0,
   "to_node": 106,
   "to_pin": 0
  },
  {
   "from_node": 101,
   "from_pin": 0,
   "to_node": 106,
   "to_pin": 1
  },
  {
   "from_node": 106,
   "from_pin": 0,
   "to_node": 107,
   "to_pin": 0
  },
  {
   "from_node": 100,
   "from_pin": 0,
   "to_node": 108,
   "to_pin": 0
  },
  {
   "from_node": 107,
   "from_pin": 4,
   "to_node": 108,
   "to_pin": 1
  },
  {
   "from_node": 21,
   "from_pin": 0,
   "to_node": 111,
   "to_pin": 0
  },
  {
   "from_node": 110,
   "from_pin": 0,
   "to_node": 111,
   "to_pin": 1
  },
  {
   "from_node": 111,
   "from_pin": 0,
   "to_node": 109,
   "to_pin": 0
  },
  {
   "from_node": 21,
   "from_pin": 1,
   "to_node": 113,
   "to_pin": 0
  },
  {
   "from_node": 112,
   "from_pin": 0,
   "to_node": 113,
   "to_pin": 1
  },
  {
   "from_node": 113,
   "from_pin": 0,
   "to_node": 109,
   "to_pin": 1
  },
  {
   "from_node": 15,
   "from_pin": 0,
   "to_node": 114,
   "to_pin": 0
  },
  {
   "from_node": 109,
   "from_pin": 0,
   "to_node": 114,
   "to_pin": 1
  },
  {
   "from_node": 114,
   "from_pin": 0,
   "to_node": 115,
   "to_pin": 0
  },
  {
   "from_node": 108,
   "from_pin": 0,
   "to_node": 116,
   "to_pin": 0
  },
  {
   "from_node": 115,
   "from_pin": 4,
   "to_node": 116,
   "to_pin": 1
  },
  {
   "from_node": 116,
   "from_pin": 0,
   "to_node": 117,
   "to_pin": 0
  },
  {
   "from_node": 16,
   "from_pin": 4,
   "to_node": 117,
   "to_pin": 1
  },
  {
   "from_node": 117,
   "from_pin": 0,
   "to_node": 119,
   "to_pin": 0
  },
  {
   "from_node": 118,
   "from_pin": 0,
   "to_node": 119,
   "to_pin": 1
  },
  {
   "from_node": 119,
   "from_pin": 0,
   "to_node": 120,
   "to_pin": 0
  },
  {
   "from_node": 4,
   "from_pin": 1,
   "to_node": 120,
   "to_pin": 1
  },
  {
   "from_node": 120,
   "from_pin": 0,
   "to_node": 121,
   "to_pin": 0
  },
  {
   "from_node": 18,
   "from_pin": 3,
   "to_node": 121,
   "to_pin": 1
  },
  {
   "from_node": 122,
   "from_pin": 0,
   "to_node": 123,
   "to_pin": 0
  },
  {
   "from_node": 124,
   "from_pin": 0,
   "to_node": 125,
   "to_pin": 0
  },
  {
   "from_node": 16,
   "from_pin": 4,
   "to_node": 125,
   "to_pin": 1
  },
  {
   "from_node": 121,
   "from_pin": 0,
   "to_node": 126,
   "to_pin": 0
  },
  {
   "from_node": 125,
   "from_pin": 0,
   "to_node": 126,
   "to_pin": 1
  },
  {
   "from_node": 16,
   "from_pin": 0,
   "to_node": 127,
   "to_pin": 0
  },
  {
   "from_node": 17,
   "from_pin": 0,
   "to_node": 127,
   "to_pin": 1
  },
  {
   "from_node": 127,
   "from_pin": 0,
   "to_node": 128,
   "to_pin": 0
  },
  {
   "from_node": 123,
   "from_pin": 0,
   "to_node": 129,
   "to_pin": 0
  },
  {
   "from_node": 126,
   "from_pin": 0,
   "to_node": 129,
   "to_pin": 1
  },
  {
   "from_node": 128,
   "from_pin": 0,
   "to_node": 130,
   "to_pin": 0
  },
  {
   "from_node": 129,
   "from_pin": 0,
   "to_node": 130,
   "to_pin": 1
  },
  {
   "from_node": 123,
   "from_pin": 1,
   "to_node": 131,
   "to_pin": 0
  },
  {
   "from_node": 126,
   "from_pin": 0,
   "to_node": 131,
   "to_pin": 1
  },
  {
   "from_node": 128,
   "from_pin": 1,
   "to_node": 132,
   "to_pin": 0
  },
  {
   "from_node": 131,
   "from_pin": 0,
   "to_node": 132,
   "to_pin": 1
  },
  {
   "from_node": 123,
   "from_pin": 2,
   "to_node": 133,
   "to_pin": 0
  },
  {
   "from_node": 126,
   "from_pin": 0,
   "to_node": 133,
   "to_pin": 1
  },
  {
   "from_node": 128,
   "from_pin": 2,
   "to_node": 134,
   "to_pin": 0
  },
  {
   "from_node": 133,
   "from_pin": 0,
   "to_node": 134,
   "to_pin": 1
  },
  {
   "from_node": 128,
   "from_pin": 3,
   "to_node": 135,
   "to_pin": 0
  },
  {
   "from_node": 126,
   "from_pin": 0,
   "to_node": 135,
   "to_pin": 1
  },
  {
   "from_node": 130,
   "from_pin": 0,
   "to_node": 136,
   "to_pin": 0
  },
  {
   "from_node": 132,
   "from_pin": 0,
   "to_node": 136,
   "to_pin": 1
  },
  {
   "from_node": 134,
   "from_pin": 0,
   "to_node": 136,
   "to_pin": 2
  },
  {
   "from_node": 135,
   "from_pin": 0,
   "to_node": 136,
   "to_pin": 3
  },
  {
   "from_node": 136,
   "from_pin": 0,
   "to_node": 0,
   "to_pin": 0
  }
 ],
 "next_id": 137
}
