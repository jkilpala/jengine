{
 "meta": {
  "name": "Ink outline",
  "description": "A cartoon's ink line round the sprite's silhouette: the texture sampled at eight neighbours `width` texels away (a Texel Size node, so the line is the same thickness on any texture), and the outline colour drawn where a neighbour is opaque and this pixel is not. The art needs that much transparent margin. `outline` is (width in texels, -, -, -); `outline_color` is its colour and alpha.",
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
    "type": "MaterialParam",
    "name": "outline"
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
    "type": "TexelSize"
   },
   "position": [
    950,
    0
   ]
  },
  {
   "id": 6,
   "node_type": {
    "type": "Multiply"
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
    "type": "Combine",
    "components": 2
   },
   "position": [
    0,
    150
   ]
  },
  {
   "id": 9,
   "node_type": {
    "type": "FloatConstant",
    "value": 1.0
   },
   "position": [
    190,
    150
   ]
  },
  {
   "id": 10,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    380,
    150
   ]
  },
  {
   "id": 11,
   "node_type": {
    "type": "FloatConstant",
    "value": 0.0
   },
   "position": [
    570,
    150
   ]
  },
  {
   "id": 12,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    760,
    150
   ]
  },
  {
   "id": 13,
   "node_type": {
    "type": "Add"
   },
   "position": [
    950,
    150
   ]
  },
  {
   "id": 14,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    1140,
    150
   ]
  },
  {
   "id": 15,
   "node_type": {
    "type": "Combine",
    "components": 2
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
    "value": -1.0
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
    "value": 0.0
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
    "type": "TextureSample"
   },
   "position": [
    950,
    300
   ]
  },
  {
   "id": 22,
   "node_type": {
    "type": "Max"
   },
   "position": [
    1140,
    300
   ]
  },
  {
   "id": 23,
   "node_type": {
    "type": "Combine",
    "components": 2
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
    "value": 0.0
   },
   "position": [
    0,
    450
   ]
  },
  {
   "id": 25,
   "node_type": {
    "type": "Multiply"
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
    "value": 1.0
   },
   "position": [
    380,
    450
   ]
  },
  {
   "id": 27,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    570,
    450
   ]
  },
  {
   "id": 28,
   "node_type": {
    "type": "Add"
   },
   "position": [
    760,
    450
   ]
  },
  {
   "id": 29,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    950,
    450
   ]
  },
  {
   "id": 30,
   "node_type": {
    "type": "Max"
   },
   "position": [
    1140,
    450
   ]
  },
  {
   "id": 31,
   "node_type": {
    "type": "Combine",
    "components": 2
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
    "type": "FloatConstant",
    "value": -1.0
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
    "type": "Add"
   },
   "position": [
    760,
    600
   ]
  },
  {
   "id": 37,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    950,
    600
   ]
  },
  {
   "id": 38,
   "node_type": {
    "type": "Max"
   },
   "position": [
    1140,
    600
   ]
  },
  {
   "id": 39,
   "node_type": {
    "type": "Combine",
    "components": 2
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
    "value": 0.7
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
    "type": "FloatConstant",
    "value": 0.7
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
    "type": "Add"
   },
   "position": [
    760,
    750
   ]
  },
  {
   "id": 45,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    950,
    750
   ]
  },
  {
   "id": 46,
   "node_type": {
    "type": "Max"
   },
   "position": [
    1140,
    750
   ]
  },
  {
   "id": 47,
   "node_type": {
    "type": "Combine",
    "components": 2
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
    "value": -0.7
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
    "type": "FloatConstant",
    "value": 0.7
   },
   "position": [
    380,
    900
   ]
  },
  {
   "id": 51,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    570,
    900
   ]
  },
  {
   "id": 52,
   "node_type": {
    "type": "Add"
   },
   "position": [
    760,
    900
   ]
  },
  {
   "id": 53,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    950,
    900
   ]
  },
  {
   "id": 54,
   "node_type": {
    "type": "Max"
   },
   "position": [
    1140,
    900
   ]
  },
  {
   "id": 55,
   "node_type": {
    "type": "Combine",
    "components": 2
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
    "type": "FloatConstant",
    "value": -0.7
   },
   "position": [
    380,
    1050
   ]
  },
  {
   "id": 59,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    570,
    1050
   ]
  },
  {
   "id": 60,
   "node_type": {
    "type": "Add"
   },
   "position": [
    760,
    1050
   ]
  },
  {
   "id": 61,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    950,
    1050
   ]
  },
  {
   "id": 62,
   "node_type": {
    "type": "Max"
   },
   "position": [
    1140,
    1050
   ]
  },
  {
   "id": 63,
   "node_type": {
    "type": "Combine",
    "components": 2
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
    "value": -0.7
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
    "type": "FloatConstant",
    "value": -0.7
   },
   "position": [
    380,
    1200
   ]
  },
  {
   "id": 67,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    570,
    1200
   ]
  },
  {
   "id": 68,
   "node_type": {
    "type": "Add"
   },
   "position": [
    760,
    1200
   ]
  },
  {
   "id": 69,
   "node_type": {
    "type": "TextureSample"
   },
   "position": [
    950,
    1200
   ]
  },
  {
   "id": 70,
   "node_type": {
    "type": "Max"
   },
   "position": [
    1140,
    1200
   ]
  },
  {
   "id": 71,
   "node_type": {
    "type": "Subtract"
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
    "value": 0.0
   },
   "position": [
    0,
    1350
   ]
  },
  {
   "id": 73,
   "node_type": {
    "type": "Max"
   },
   "position": [
    190,
    1350
   ]
  },
  {
   "id": 74,
   "node_type": {
    "type": "MaterialParam",
    "name": "outline_color"
   },
   "position": [
    380,
    1350
   ]
  },
  {
   "id": 75,
   "node_type": {
    "type": "Split"
   },
   "position": [
    570,
    1350
   ]
  },
  {
   "id": 76,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    760,
    1350
   ]
  },
  {
   "id": 77,
   "node_type": {
    "type": "VertexColorInput"
   },
   "position": [
    950,
    1350
   ]
  },
  {
   "id": 78,
   "node_type": {
    "type": "Multiply"
   },
   "position": [
    1140,
    1350
   ]
  },
  {
   "id": 79,
   "node_type": {
    "type": "Mix"
   },
   "position": [
    1330,
    1350
   ]
  },
  {
   "id": 80,
   "node_type": {
    "type": "Split"
   },
   "position": [
    0,
    1500
   ]
  },
  {
   "id": 81,
   "node_type": {
    "type": "Max"
   },
   "position": [
    190,
    1500
   ]
  },
  {
   "id": 82,
   "node_type": {
    "type": "Combine",
    "components": 4
   },
   "position": [
    380,
    1500
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
   "from_node": 3,
   "from_pin": 0,
   "to_node": 4,
   "to_pin": 0
  },
  {
   "from_node": 5,
   "from_pin": 0,
   "to_node": 6,
   "to_pin": 0
  },
  {
   "from_node": 4,
   "from_pin": 0,
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
   "to_node": 8,
   "to_pin": 0
  },
  {
   "from_node": 7,
   "from_pin": 1,
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
   "to_node": 8,
   "to_pin": 1
  },
  {
   "from_node": 1,
   "from_pin": 0,
   "to_node": 13,
   "to_pin": 0
  },
  {
   "from_node": 8,
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
   "from_node": 7,
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
   "to_node": 15,
   "to_pin": 0
  },
  {
   "from_node": 7,
   "from_pin": 1,
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
   "from_node": 19,
   "from_pin": 0,
   "to_node": 15,
   "to_pin": 1
  },
  {
   "from_node": 1,
   "from_pin": 0,
   "to_node": 20,
   "to_pin": 0
  },
  {
   "from_node": 15,
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
   "from_node": 14,
   "from_pin": 4,
   "to_node": 22,
   "to_pin": 0
  },
  {
   "from_node": 21,
   "from_pin": 4,
   "to_node": 22,
   "to_pin": 1
  },
  {
   "from_node": 7,
   "from_pin": 0,
   "to_node": 25,
   "to_pin": 0
  },
  {
   "from_node": 24,
   "from_pin": 0,
   "to_node": 25,
   "to_pin": 1
  },
  {
   "from_node": 25,
   "from_pin": 0,
   "to_node": 23,
   "to_pin": 0
  },
  {
   "from_node": 7,
   "from_pin": 1,
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
   "to_node": 23,
   "to_pin": 1
  },
  {
   "from_node": 1,
   "from_pin": 0,
   "to_node": 28,
   "to_pin": 0
  },
  {
   "from_node": 23,
   "from_pin": 0,
   "to_node": 28,
   "to_pin": 1
  },
  {
   "from_node": 28,
   "from_pin": 0,
   "to_node": 29,
   "to_pin": 0
  },
  {
   "from_node": 22,
   "from_pin": 0,
   "to_node": 30,
   "to_pin": 0
  },
  {
   "from_node": 29,
   "from_pin": 4,
   "to_node": 30,
   "to_pin": 1
  },
  {
   "from_node": 7,
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
   "from_node": 33,
   "from_pin": 0,
   "to_node": 31,
   "to_pin": 0
  },
  {
   "from_node": 7,
   "from_pin": 1,
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
   "from_node": 35,
   "from_pin": 0,
   "to_node": 31,
   "to_pin": 1
  },
  {
   "from_node": 1,
   "from_pin": 0,
   "to_node": 36,
   "to_pin": 0
  },
  {
   "from_node": 31,
   "from_pin": 0,
   "to_node": 36,
   "to_pin": 1
  },
  {
   "from_node": 36,
   "from_pin": 0,
   "to_node": 37,
   "to_pin": 0
  },
  {
   "from_node": 30,
   "from_pin": 0,
   "to_node": 38,
   "to_pin": 0
  },
  {
   "from_node": 37,
   "from_pin": 4,
   "to_node": 38,
   "to_pin": 1
  },
  {
   "from_node": 7,
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
   "from_node": 41,
   "from_pin": 0,
   "to_node": 39,
   "to_pin": 0
  },
  {
   "from_node": 7,
   "from_pin": 1,
   "to_node": 43,
   "to_pin": 0
  },
  {
   "from_node": 42,
   "from_pin": 0,
   "to_node": 43,
   "to_pin": 1
  },
  {
   "from_node": 43,
   "from_pin": 0,
   "to_node": 39,
   "to_pin": 1
  },
  {
   "from_node": 1,
   "from_pin": 0,
   "to_node": 44,
   "to_pin": 0
  },
  {
   "from_node": 39,
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
   "from_node": 38,
   "from_pin": 0,
   "to_node": 46,
   "to_pin": 0
  },
  {
   "from_node": 45,
   "from_pin": 4,
   "to_node": 46,
   "to_pin": 1
  },
  {
   "from_node": 7,
   "from_pin": 0,
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
   "to_node": 47,
   "to_pin": 0
  },
  {
   "from_node": 7,
   "from_pin": 1,
   "to_node": 51,
   "to_pin": 0
  },
  {
   "from_node": 50,
   "from_pin": 0,
   "to_node": 51,
   "to_pin": 1
  },
  {
   "from_node": 51,
   "from_pin": 0,
   "to_node": 47,
   "to_pin": 1
  },
  {
   "from_node": 1,
   "from_pin": 0,
   "to_node": 52,
   "to_pin": 0
  },
  {
   "from_node": 47,
   "from_pin": 0,
   "to_node": 52,
   "to_pin": 1
  },
  {
   "from_node": 52,
   "from_pin": 0,
   "to_node": 53,
   "to_pin": 0
  },
  {
   "from_node": 46,
   "from_pin": 0,
   "to_node": 54,
   "to_pin": 0
  },
  {
   "from_node": 53,
   "from_pin": 4,
   "to_node": 54,
   "to_pin": 1
  },
  {
   "from_node": 7,
   "from_pin": 0,
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
   "to_node": 55,
   "to_pin": 0
  },
  {
   "from_node": 7,
   "from_pin": 1,
   "to_node": 59,
   "to_pin": 0
  },
  {
   "from_node": 58,
   "from_pin": 0,
   "to_node": 59,
   "to_pin": 1
  },
  {
   "from_node": 59,
   "from_pin": 0,
   "to_node": 55,
   "to_pin": 1
  },
  {
   "from_node": 1,
   "from_pin": 0,
   "to_node": 60,
   "to_pin": 0
  },
  {
   "from_node": 55,
   "from_pin": 0,
   "to_node": 60,
   "to_pin": 1
  },
  {
   "from_node": 60,
   "from_pin": 0,
   "to_node": 61,
   "to_pin": 0
  },
  {
   "from_node": 54,
   "from_pin": 0,
   "to_node": 62,
   "to_pin": 0
  },
  {
   "from_node": 61,
   "from_pin": 4,
   "to_node": 62,
   "to_pin": 1
  },
  {
   "from_node": 7,
   "from_pin": 0,
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
   "to_node": 63,
   "to_pin": 0
  },
  {
   "from_node": 7,
   "from_pin": 1,
   "to_node": 67,
   "to_pin": 0
  },
  {
   "from_node": 66,
   "from_pin": 0,
   "to_node": 67,
   "to_pin": 1
  },
  {
   "from_node": 67,
   "from_pin": 0,
   "to_node": 63,
   "to_pin": 1
  },
  {
   "from_node": 1,
   "from_pin": 0,
   "to_node": 68,
   "to_pin": 0
  },
  {
   "from_node": 63,
   "from_pin": 0,
   "to_node": 68,
   "to_pin": 1
  },
  {
   "from_node": 68,
   "from_pin": 0,
   "to_node": 69,
   "to_pin": 0
  },
  {
   "from_node": 62,
   "from_pin": 0,
   "to_node": 70,
   "to_pin": 0
  },
  {
   "from_node": 69,
   "from_pin": 4,
   "to_node": 70,
   "to_pin": 1
  },
  {
   "from_node": 70,
   "from_pin": 0,
   "to_node": 71,
   "to_pin": 0
  },
  {
   "from_node": 2,
   "from_pin": 4,
   "to_node": 71,
   "to_pin": 1
  },
  {
   "from_node": 71,
   "from_pin": 0,
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
   "from_node": 74,
   "from_pin": 0,
   "to_node": 75,
   "to_pin": 0
  },
  {
   "from_node": 73,
   "from_pin": 0,
   "to_node": 76,
   "to_pin": 0
  },
  {
   "from_node": 75,
   "from_pin": 3,
   "to_node": 76,
   "to_pin": 1
  },
  {
   "from_node": 2,
   "from_pin": 0,
   "to_node": 78,
   "to_pin": 0
  },
  {
   "from_node": 77,
   "from_pin": 0,
   "to_node": 78,
   "to_pin": 1
  },
  {
   "from_node": 74,
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
   "from_node": 2,
   "from_pin": 4,
   "to_node": 79,
   "to_pin": 2
  },
  {
   "from_node": 79,
   "from_pin": 0,
   "to_node": 80,
   "to_pin": 0
  },
  {
   "from_node": 2,
   "from_pin": 4,
   "to_node": 81,
   "to_pin": 0
  },
  {
   "from_node": 76,
   "from_pin": 0,
   "to_node": 81,
   "to_pin": 1
  },
  {
   "from_node": 80,
   "from_pin": 0,
   "to_node": 82,
   "to_pin": 0
  },
  {
   "from_node": 80,
   "from_pin": 1,
   "to_node": 82,
   "to_pin": 1
  },
  {
   "from_node": 80,
   "from_pin": 2,
   "to_node": 82,
   "to_pin": 2
  },
  {
   "from_node": 81,
   "from_pin": 0,
   "to_node": 82,
   "to_pin": 3
  },
  {
   "from_node": 82,
   "from_pin": 0,
   "to_node": 0,
   "to_pin": 0
  }
 ],
 "next_id": 83
}
