-- Walking a hero round a 3D stage, shared by every scene that has one.
--
-- The hero's place is kept here, in stage units (x east, y north), and
-- handed to the stage with Stage.place each frame: the stage owns the
-- hero's transform. "Up" walks away from the camera whatever way it is
-- turned, so the controls stay the screen's as Q / E orbit the view. The
-- footprints in `blocked` (houses, the well) are walked round by sliding
-- along them, an axis at a time.
--
-- NPCs walk a loop of points at their own pace, pausing at each.

local M = {}

local function inside(b, x, y, r)
    return x > b[1] - r and x < b[3] + r and y > b[2] - r and y < b[4] + r
end

local function free(self, x, y)
    local r = 0.35
    local b = self.bounds
    if x < b[1] or x > b[3] or y < b[2] or y > b[4] then return false end
    for _, box in ipairs(self.blocked) do
        if inside(box, x, y, r) then return false end
    end
    return true
end

-- opts: hero (name), stage (name), x, y, bounds {x0, y0, x1, y1},
-- blocked (list of {x0, y0, x1, y1}), speed, yaw, turn (radians a second;
-- 0 keeps the camera still), yaw_limit (how far either way it may turn),
-- tour (a loop of {x, y} the hero walks by itself once no key has been held
-- for `idle_after` seconds - so a scene left alone shows itself off, and a
-- capture with no hand on the keys still sees the hero walk).
function M.new(opts)
    local self = {
        hero = Entity.find_by_name(opts.hero or "Hero"),
        stage = Entity.find_by_name(opts.stage or "Stage"),
        x = opts.x or 0, y = opts.y or 0,
        bounds = opts.bounds or { -20, -20, 20, 20 },
        blocked = opts.blocked or {},
        speed = opts.speed or 3.2,
        yaw = opts.yaw or 0,
        turn = opts.turn or 1.2,
        yaw0 = opts.yaw or 0, yaw_limit = opts.yaw_limit,
        facing = 1, walking = false,
        npcs = {},
        tour = opts.tour, tour_i = 1, idle = 0, idle_after = opts.idle_after or 8, stuck = 0,
    }
    if self.hero then Stage.place(self.hero, self.x, self.y, 0) end
    return self
end

-- The screen direction that walks the hero toward the tour's next point.
function M.tour_step(self, dt)
    local p = self.tour[self.tour_i]
    local wx, wy = p[1] - self.x, p[2] - self.y
    local d = math.sqrt(wx * wx + wy * wy)
    self.stuck = self.stuck + dt
    if d < 0.3 or self.stuck > 6 then
        self.tour_i = self.tour_i % #self.tour + 1
        self.stuck = 0
        return 0, 0
    end
    wx, wy = wx / d, wy / d
    -- The inverse of the walk's screen-to-ground turn.
    local s, c = math.sin(self.yaw), math.cos(self.yaw)
    return wx * c - wy * s, wx * s + wy * c
end

local function move_dir()
    local dx, dy = 0, 0
    if Input.is_action_pressed("move_left") then dx = dx - 1 end
    if Input.is_action_pressed("move_right") then dx = dx + 1 end
    if Input.is_action_pressed("move_up") then dy = dy + 1 end
    if Input.is_action_pressed("move_down") then dy = dy - 1 end
    return dx, dy
end

function M.update(self, dt)
    -- Turn the camera.
    if self.turn > 0 and self.stage then
        local t = 0
        if Input.is_action_pressed("turn_left") then t = t - 1 end
        if Input.is_action_pressed("turn_right") then t = t + 1 end
        if t ~= 0 then
            self.yaw = self.yaw + t * self.turn * dt
            if self.yaw_limit then
                self.yaw = math.max(self.yaw0 - self.yaw_limit, math.min(self.yaw0 + self.yaw_limit, self.yaw))
            end
            Stage.set(self.stage, "yaw", self.yaw)
        end
    end
    if not self.hero then
        for _, n in ipairs(self.npcs) do M.step_npc(n, dt, self.yaw) end
        return
    end
    local dx, dy = move_dir()
    if dx ~= 0 or dy ~= 0 then
        self.idle = 0
    else
        self.idle = self.idle + dt
        if self.tour and self.idle > self.idle_after then
            dx, dy = M.tour_step(self, dt)
        end
    end
    local walking = dx ~= 0 or dy ~= 0
    if walking then
        local len = math.sqrt(dx * dx + dy * dy)
        dx, dy = dx / len, dy / len
        -- Screen right and screen up, on the ground, for this yaw.
        local s, c = math.sin(self.yaw), math.cos(self.yaw)
        local wx = dx * c + dy * s
        local wy = -dx * s + dy * c
        local step = self.speed * dt
        local nx, ny = self.x + wx * step, self.y + wy * step
        if free(self, nx, self.y) then self.x = nx end
        if free(self, self.x, ny) then self.y = ny end
        if dx ~= 0 then
            local f = dx > 0 and 1 or -1
            if f ~= self.facing then
                self.facing = f
                Stage.set(self.hero, "scale", { f, 1 })
            end
        end
        Stage.place(self.hero, self.x, self.y, 0)
    end
    if walking ~= self.walking then
        self.walking = walking
        Animation.play(self.hero, walking and "walk" or "idle")
    end
    for _, n in ipairs(self.npcs) do M.step_npc(n, dt, self.yaw) end
end

-- An NPC walking a loop of {x, y} points at `speed`, pausing `pause`
-- seconds at each.
function M.npc(self, name, points, speed, pause)
    local e = Entity.find_by_name(name)
    if not e then return end
    local n = { e = e, pts = points, i = 1, x = points[1][1], y = points[1][2], speed = speed or 1.4,
                pause = pause or 1.5, wait = math.random() * 2, facing = 1, walking = false }
    Stage.place(e, n.x, n.y, 0)
    self.npcs[#self.npcs + 1] = n
    return n
end

function M.step_npc(n, dt, yaw)
    if n.wait > 0 then
        n.wait = n.wait - dt
        if n.walking then
            n.walking = false
            Animation.play(n.e, "idle")
        end
        return
    end
    local target = n.pts[n.i % #n.pts + 1]
    local dx, dy = target[1] - n.x, target[2] - n.y
    local d = math.sqrt(dx * dx + dy * dy)
    if d < 0.05 then
        n.i = n.i % #n.pts + 1
        n.wait = n.pause
        return
    end
    if not n.walking then
        n.walking = true
        Animation.play(n.e, "walk")
    end
    local step = math.min(d, n.speed * dt)
    n.x, n.y = n.x + dx / d * step, n.y + dy / d * step
    -- Face the way it walks as the screen sees it: along the camera's right.
    yaw = yaw or 0
    local f = (dx * math.cos(yaw) - dy * math.sin(yaw)) >= 0 and 1 or -1
    if f ~= n.facing then
        n.facing = f
        Stage.set(n.e, "scale", { f, 1 })
    end
    Stage.place(n.e, n.x, n.y, 0)
end

-- Post effects a scene can toggle with 1-5.
M.POST = { "depth_of_field", "bloom", "vignette", "color_grade", "depth_haze" }
M.POST_LABEL = { "depth of field", "bloom", "vignette", "grade", "haze" }

function M.post_keys(on)
    local changed = false
    for i, name in ipairs(M.POST) do
        if Input.is_key_just_pressed(tostring(i)) then
            on[name] = not on[name]
            PostProcess.set_enabled(name, on[name])
            changed = true
        end
    end
    return changed
end

function M.post_status(on)
    local parts = {}
    for i, name in ipairs(M.POST) do
        parts[#parts + 1] = i .. " " .. M.POST_LABEL[i] .. (on[name] and " on" or " off")
    end
    return table.concat(parts, "   ")
end

function M.post_all_on()
    local on = {}
    for _, name in ipairs(M.POST) do on[name] = true end
    return on
end

return M
