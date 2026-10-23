{
 "meta": {
  "name": "Scene color: lens",
  "description": "What is behind, shifted a fixed 4% of the screen to the right.",
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
    "type": "Vec2Constant",
    "value": [
     0.04,
     0.0
    ]
   },
   "position": [
    190,
    0
   ]
  },
  {
   "id": 2,
   "node_type": {
    "type": "SceneColor"
   },
   "position": [
    380,
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
   "from_node": 2,
   "from_pin": 0,
   "to_node": 0,
   "to_pin": 0
  }
 ],
 "next_id": 3
}
