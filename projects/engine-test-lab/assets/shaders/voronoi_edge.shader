{
 "meta": {
  "name": "Voronoi Edge",
  "description": "The Voronoi node's Edge output, 6 cells across.",
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
    "type": "Voronoi"
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
    "value": 6.0
   },
   "position": [
    570,
    0
   ]
  },
  {
   "id": 4,
   "node_type": {
    "type": "FloatConstant",
    "value": 1.0
   },
   "position": [
    760,
    0
   ]
  },
  {
   "id": 5,
   "node_type": {
    "type": "Combine",
    "components": 4
   },
   "position": [
    950,
    0
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
   "to_node": 2,
   "to_pin": 1
  },
  {
   "from_node": 2,
   "from_pin": 2,
   "to_node": 5,
   "to_pin": 0
  },
  {
   "from_node": 2,
   "from_pin": 2,
   "to_node": 5,
   "to_pin": 1
  },
  {
   "from_node": 2,
   "from_pin": 2,
   "to_node": 5,
   "to_pin": 2
  },
  {
   "from_node": 4,
   "from_pin": 0,
   "to_node": 5,
   "to_pin": 3
  },
  {
   "from_node": 5,
   "from_pin": 0,
   "to_node": 0,
   "to_pin": 0
  }
 ],
 "next_id": 6
}
