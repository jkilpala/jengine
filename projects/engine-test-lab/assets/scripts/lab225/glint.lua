-- Scene 225: a module the gem script requires, from pack lab225.
local M = {}

function M.pulse(t)
    return 1 + 0.08 * math.sin(t * 4)
end

-- -1..1, a different place for each spark.
function M.offset(t)
    return math.sin(t * 2.3)
end

return M
