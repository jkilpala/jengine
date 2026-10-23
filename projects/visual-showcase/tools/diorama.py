"""Tilt-Shift Diorama (showcase scene 23): a small town seen from above.

One layout, read by the texture generator (the ground: grass, roads, a
river, a park, fields) and the scene generator (houses, trees, cars on
their loops, boats, smoke) so the roads the cars drive are the roads that
are painted. World units are the ground image's pixels, centred on 0,
y up.
"""

import math
import random

GROUND_W, GROUND_H = 1760, 1040
ROAD_W = 46.0
LANE = 11.0

# Roads: horizontal at these y, vertical at these x (east of the river).
ROADS_Y = [200.0, -170.0]
ROADS_X = [-360.0, 90.0, 560.0]
X_MIN, X_MAX = -560.0, 860.0          # where the grid runs (the river is west of X_MIN)
Y_MIN, Y_MAX = -480.0, 480.0


def river_x(y):
    """The river's centre line, meandering down the west of the town."""
    return -700.0 + 60.0 * math.sin(y / 190.0) + 25.0 * math.sin(y / 70.0 + 1.0)


RIVER_W = 120.0
PARK = (90.0 + ROAD_W / 2 + 18, -170.0 + ROAD_W / 2 + 18, 560.0 - ROAD_W / 2 - 18, 200.0 - ROAD_W / 2 - 18)
POND = (330.0, 20.0, 95.0, 60.0)        # centre x, y, radii


def blocks():
    """The rectangles between roads, as (x0, y0, x1, y1), minus road width."""
    xs = [X_MIN] + ROADS_X + [X_MAX]
    ys = [Y_MIN] + ROADS_Y + [Y_MAX]
    ys.sort()
    out = []
    for i in range(len(xs) - 1):
        for j in range(len(ys) - 1):
            x0, x1 = xs[i] + ROAD_W / 2, xs[i + 1] - ROAD_W / 2
            y0, y1 = ys[j] + ROAD_W / 2, ys[j + 1] - ROAD_W / 2
            out.append((x0, y0, x1, y1))
    return out


def is_park(b):
    return abs(b[0] - PARK[0]) < 30 and abs(b[1] - PARK[1]) < 30


def houses(seed=23):
    """Houses along the streets of every block but the park: (x, y, w, h,
    roof style, facing) - facing is the street side, which the roof's
    ridge runs along."""
    rng = random.Random(seed)
    out = []
    for b in blocks():
        if is_park(b):
            continue
        x0, y0, x1, y1 = b
        # A row along the top and bottom edges of the block.
        for edge_y, face in ((y1 - 38, "n"), (y0 + 38, "s")):
            x = x0 + 30
            while x < x1 - 40:
                w = rng.choice((56, 64, 72, 84))
                if x + w / 2 > x1 - 20:
                    break
                if rng.random() < 0.85:
                    out.append((x + w / 2, edge_y, w, rng.choice((52, 58, 64)), rng.randrange(6), face, 0.0))
                x += w + rng.choice((10, 16, 22))
        # And a row down each side street, turned to face it, where the
        # block is tall enough to leave room between the corner houses.
        for edge_x in (x0 + 36, x1 - 36):
            y = y0 + 110
            while y < y1 - 110:
                h = rng.choice((52, 60, 68))
                if rng.random() < 0.8:
                    out.append((edge_x, y + h / 2, h, rng.choice((50, 56)), rng.randrange(6), "e", 1.5708))
                y += h + rng.choice((12, 18))
    return out


def trees(seed=5):
    """Trees: the park full of them, gardens behind the houses, a line
    along the river."""
    rng = random.Random(seed)
    out = []
    px0, py0, px1, py1 = PARK
    for _ in range(46):
        x, y = rng.uniform(px0, px1), rng.uniform(py0, py1)
        if ((x - POND[0]) / (POND[2] + 30)) ** 2 + ((y - POND[1]) / (POND[3] + 30)) ** 2 < 1:
            continue
        if abs(y - 18) < 16:        # the park path
            continue
        out.append((x, y, rng.uniform(22, 36), rng.randrange(3)))
    for b in blocks():
        if is_park(b):
            continue
        x0, y0, x1, y1 = b
        for _ in range(int((x1 - x0) / 30)):
            out.append((rng.uniform(x0 + 20, x1 - 20), rng.uniform((y0 + y1) / 2 - 22, (y0 + y1) / 2 + 22),
                        rng.uniform(16, 26), rng.randrange(3)))
    y = Y_MIN
    while y < Y_MAX:
        for side in (-1, 1):
            if rng.random() < 0.7:
                out.append((river_x(y) + side * (RIVER_W / 2 + 26 + rng.uniform(0, 30)), y, rng.uniform(18, 30),
                            rng.randrange(3)))
        y += rng.uniform(40, 70)
    return out


def car_loops():
    """Loops around blocks, in the right-hand lane: (points, looped)."""
    loops = {}

    def ring(name, x0, y0, x1, y1, clockwise):
        # Road centre lines around the block, offset into one lane.
        o = LANE if clockwise else -LANE
        pts = [(x0 + o, y0 + o), (x0 + o, y1 - o), (x1 - o, y1 - o), (x1 - o, y0 + o)]
        if not clockwise:
            pts = pts[::-1]
        loops[name] = {"points": [[round(x, 1), round(y, 1)] for x, y in pts], "looped": True}

    ring("td_loop_a", ROADS_X[0], ROADS_Y[1], ROADS_X[1], ROADS_Y[0], True)
    ring("td_loop_b", ROADS_X[1], ROADS_Y[1], ROADS_X[2], ROADS_Y[0], False)
    ring("td_loop_c", ROADS_X[0], ROADS_Y[1], ROADS_X[2], ROADS_Y[0], False)
    # Through traffic: rings whose return legs run off screen - east along
    # one road, west along the other (each in its right-hand lane), north
    # up one avenue, south down another.
    def ring_pts(name, pts):
        loops[name] = {"points": [[round(x, 1), round(y, 1)] for x, y in pts], "looped": True}
    ring_pts("td_thru_ew", [(-980.0, ROADS_Y[0] - LANE), (1000.0, ROADS_Y[0] - LANE),
                            (1000.0, ROADS_Y[1] + LANE), (-980.0, ROADS_Y[1] + LANE)])
    ring_pts("td_thru_ns", [(ROADS_X[2] + LANE, -640.0), (ROADS_X[2] + LANE, 640.0),
                            (ROADS_X[0] - LANE, 640.0), (ROADS_X[0] - LANE, -640.0)])
    # Boats: up and down the river.
    ys = [Y_MIN - 60 + i * 60 for i in range(int((Y_MAX - Y_MIN + 120) / 60) + 1)]
    loops["td_river"] = {"points": [[round(river_x(y) + 18, 1), round(y, 1)] for y in ys], "looped": False}
    loops["td_river_back"] = {"points": [[round(river_x(y) - 18, 1), round(y, 1)] for y in reversed(ys)],
                              "looped": False}
    return loops
