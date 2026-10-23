-- Scene 221 - Online zone (jengine-online against jengine-server)
--
-- The game's side of a server-run world. The server decides everything: a
-- click here is a *request* to walk somewhere, and what moves on screen is
-- what the server then says happened.
--
-- Start a server first:   cargo run -p jengine-server --example zones
-- and, for company:       cargo run -p jengine-server --example bots -- ws://127.0.0.1:9001 6
--
-- What is authored and what the package does:
--
--   "Online"            the connection (`online` component). Its `online` bag
--                       is what this script reads: status, room, events.
--   "Player 1".."8"     a pool of `online_slot`s of kind "player"
--   "Coin 1".."6"       ... and of kind "coin"
--   "Wall 1".."30"      ... of kind "wall": the server's tilemap, one block a cell
--   "Monster 1".."3"    ... of kind "monster": they hunt on the server, with the
--                       engine's own A* over that tilemap, and bite a coin off you
--
-- When the server says a player is in view, a free player slot is bound to
-- it: moved there every frame (walked between the server's ticks, so it is
-- smooth at any frame rate) and given a `net_entity` bag with what the room
-- set on it. Nothing here moves a sprite.
--
-- Two ways to move, and they are different things. A click is a request the
-- server walks for you (`Online.send { go = .. }`): nothing moves until it
-- says so. WASD / the arrows steer with `Online.input`, which is
-- **predicted**: the avatar moves this frame, the server makes the same
-- step when the input reaches it, and the package reconciles the two. The
-- status line shows how many inputs the server has not answered yet.
--
-- The login token is kept with the save system (`Save.set` on `welcome`), so
-- the next launch is the same player.
--
-- Nothing connects on load. C connects; the component already names the room
-- to join, so signing in and joining follow by themselves. Walk off the east
-- edge and the server moves you to the next zone: the only thing that arrives
-- is a new `joined`, and the slots are re-bound to another room's entities.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/221-online-zone.scene"

local SAVE_SLOT = "online"
local WIDTH, HEIGHT = 1000, 600
local GROUND = {
    meadow = { 0.03, 0.09, 0.05 },
    cave = { 0.05, 0.05, 0.09 },
}

local conn, status_text, ground, room_name
local players, labels = {}, {}
local coins = 0
local said = ""
local steering = false

function init(entity)
    conn = Entity.find_by_name("Online")
    status_text = Entity.find_by_name("Status")
    ground = Entity.find_by_name("Ground")
    room_name = Entity.find_by_name("RoomName")
    for i = 1, 8 do
        players[i] = Entity.find_by_name("Player " .. i)
        labels[i] = Entity.find_by_name("Player " .. i .. " Label")
    end
    log("[scene_221] ready - press C to connect to ws://127.0.0.1:9001")
end

local function bag(field)
    return Entity.get_custom(conn, "online", field)
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if conn == nil then return end
    if Online == nil then
        Text.set(status_text, "jengine-online is not linked into this build: rebuild the project's runtime or viewport")
        return
    end

    if Input.is_key_just_pressed("C") then
        -- The token from last time is who we are. Without it every launch
        -- would be a new player; with it, the same one, where they were.
        local token = Save.get(SAVE_SLOT, "token") or ""
        Online.connect(conn, "", "", token)    -- empty: what the component says
    elseif Input.is_key_just_pressed("X") then
        Online.disconnect(conn)
    elseif Input.is_key_just_pressed("L") then
        Online.leave(conn)
    elseif Input.is_key_just_pressed("J") then
        Online.join(conn, "zone", "meadow")
    end

    local status = bag("status") or "offline"

    -- A click asks the server to walk us there. The body is a table; what
    -- it means is the room's business, not the package's.
    if status == "joined" and Input.is_mouse_left_just_pressed() then
        local wx, wy = Input.get_mouse_world_position()
        if wx ~= nil and wy > -20 and wy < HEIGHT + 20 and wx > -40 and wx < WIDTH + 40 then
            Online.send(conn, { go = { wx, wy } })
        end
    end

    -- Steering: sent every frame while joined, zeros included - a stick let
    -- go is an input too, and it is what stops the avatar.
    if status == "joined" then
        local sx, sy = 0, 0
        if Input.is_key_pressed("D") or Input.is_key_pressed("Right") then sx = sx + 1 end
        if Input.is_key_pressed("A") or Input.is_key_pressed("Left") then sx = sx - 1 end
        if Input.is_key_pressed("W") or Input.is_key_pressed("Up") then sy = sy + 1 end
        if Input.is_key_pressed("S") or Input.is_key_pressed("Down") then sy = sy - 1 end
        if sx ~= 0 or sy ~= 0 or steering then
            Online.input(conn, sx, sy)
        end
        steering = sx ~= 0 or sy ~= 0
    end

    -- Every frame the server sent this frame, except `sync`, as a table.
    for _, event in ipairs(bag("events") or {}) do
        if event.t == "coins" or event.t == "joined" then
            coins = event.coins or coins
        elseif event.t == "bitten" then
            coins = event.coins or coins
            said = "bitten! a coin gone"
        elseif event.t == "error" then
            said = "refused: " .. tostring(event.why)
        elseif event.t == "notice" then
            said = "notice: " .. tostring(event.text)
        elseif event.t == "kicked" then
            said = "kicked: " .. tostring(event.why)
        elseif event.t == "dropped" then
            said = "connection lost - retrying"
        elseif event.t == "welcome" then
            said = ""
            -- Keep who we are for the next launch.
            Save.set(SAVE_SLOT, "token", event.token)
            Save.commit(SAVE_SLOT)
            log("[scene_221] signed in as " .. tostring(event.player))
        end
    end

    -- The slots are already where they should be. All that is left is what
    -- only the game knows: how to draw "this one is me", and the names.
    for i = 1, 8 do
        local name = Entity.get_custom(players[i], "net_entity", "name")
        if name ~= nil then
            Text.set(labels[i], name)
            if Entity.get_custom(players[i], "net_entity", "mine") then
                Sprite.set_color(players[i], 1.0, 0.9, 0.3, 1.0)
            else
                Sprite.set_color(players[i], 0.35, 0.6, 1.0, 1.0)
            end
        end
    end

    local room = bag("room")
    local where = "no room"
    if room ~= nil then
        where = room.kind .. "/" .. room.id
        Text.set(room_name, room.id)
        local c = GROUND[room.id] or GROUND.meadow
        Sprite.set_color(ground, c[1], c[2], c[3], 1.0)
    else
        Text.set(room_name, "")
    end

    local line
    if status == "offline" then
        line = "offline (" .. tostring(bag("net") or "") .. ") - press C. Nothing connects on its own."
    else
        line = string.format("%s   %s   %s   coins %d   in view %d   not shown %d   unanswered inputs %d",
            status, tostring(bag("name") or ""), where,
            math.floor(coins + 0.5), math.floor((bag("count") or 0) + 0.5), math.floor((bag("overflow") or 0) + 0.5),
            math.floor((bag("unacknowledged") or 0) + 0.5))
    end
    if said ~= "" then line = line .. "\n" .. said end
    Text.set(status_text, line)
end
