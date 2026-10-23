-- Scene 100: behaviour graphs and scripts share one event bus.
--
-- Three things that did not work anywhere before Sep 2026, side by side:
--
-- * Left - a script's `send_event` reaching a graph. The lamps' graph listens
--   with `OnEvent("lamp_level")` and multiplies the number it is sent into a
--   brightness, which is also arithmetic on a trigger's output - that did
--   not compile before either.
-- * Middle - a graph's event reaching a script, and `OnDespawn` knowing
--   where its entity was. K despawns a pod from here; the pod's graph drops a
--   ring at `GetPosition` and emits `pod_gone`, heard below with x, y. The
--   ring used to land at the origin and the event never arrived.
-- * Right - a trigger's second output. `OnMouseClick.button` could not be
--   wired: only the first output of any node could.
--
-- Compare native / editor / web: the ring must sit where the pod was, and
-- the readout must name the same coordinates, in all three.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/100-graph-events.scene"

local readout
local pods = {}
local heard = 0

function init(entity)
    readout = Entity.find_by_name("Readout")
    for i = 0, 2 do
        pods[#pods + 1] = Entity.find_by_name("Pod" .. i)
    end
    Event.on("pod_gone", entity)
    Text.set(readout, "Press K to despawn a pod.")
    log("[100-graph-events] ready")
end

function update(entity, dt)
    for n = 1, 3 do
        if Input.is_key_just_pressed(tostring(n)) then
            Event.send("lamp_level", entity, n)
        end
    end
    if Input.is_key_just_pressed("K") and #pods > 0 then
        local pod = table.remove(pods, 1)
        Entity.destroy(pod)
    end
    nav.tick(SCENE_PATH)
end

-- `data` is `{ value, x, y }`: the graph's value pin, and where the pod was
-- when its `OnDespawn` ran - after the entity had already gone.
function on_pod_gone(entity, data)
    heard = heard + 1
    if data == nil then
        Text.set(readout, "heard pod_gone with no data - the payload did not arrive")
        return
    end
    local x = math.floor((data.x or 0) + 0.5)
    local y = math.floor((data.y or 0) + 0.5)
    Text.set(readout, "heard pod_gone #" .. heard .. ": value " ..
        math.floor((data.value or 0) + 0.5) .. " at (" .. x .. ", " .. y .. ")" ..
        ((x == 0 and y == 0) and "   <- THE ORIGIN: OnDespawn lost its position" or ""))
end
