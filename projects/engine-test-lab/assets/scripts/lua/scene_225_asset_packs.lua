-- Scene 225 - asset packs
--
-- Pack lab225 (settings.asset_packs) takes scenes/lab225/, sprites/lab225/,
-- scripts/lab225/, prefabs/Lab225Spark.prefab and data/lab225_vault.data. A
-- build moves them out of the game into packs/lab225-<hash>.jpak; Packs.download fetches, checks and mounts it,
-- and the room it carries is laid in from it with Scene.load_additive - its
-- gem runs scripts/lab225/gem.lua, which needs the pack's module, table and
-- prefab, so it shows each reached the runtime with the pack. In
-- the editor - or a run from the project folder - every pack is local, so
-- it is "installed" from the first frame and the room is there at once.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/225-asset-packs.scene"
local PACK = "lab225"

local status_ui, detail_ui
local chunk = 0

function init(entity)
    status_ui = Entity.find_by_name("Status")
    detail_ui = Entity.find_by_name("Detail")
    chunk = 0
end

local function kb(bytes)
    return math.floor(bytes / 1024 + 0.5)
end

function update(entity, dt)
    if Input.is_key_just_pressed("D") then
        Packs.download(PACK)
    end
    if Input.is_key_just_pressed("R") then
        if chunk ~= 0 then
            Scene.unload(chunk)
            chunk = 0
        end
        Packs.remove(PACK)
    end

    local status = Packs.status(PACK)
    if status == "installed" and chunk == 0 then
        chunk = Scene.load_additive("scenes/lab225/vault.scene", 0, -80)
    end

    local percent = math.floor(Packs.progress(PACK) * 100 + 0.5)
    Text.set(status_ui, "pack '" .. PACK .. "': " .. status .. "  " .. percent .. "%")
    local owner = Packs.of("sprites/lab225/gem.png")
    local detail = "packs: " .. table.concat(Packs.list(), ", ")
        .. "    download: " .. kb(Packs.size(PACK)) .. " KB"
        .. "    gem.png is in pack: '" .. owner .. "' ('' when local)"
    local err = Packs.error(PACK)
    if err ~= "" then
        detail = detail .. "\nlast error: " .. err
    end
    Text.set(detail_ui, detail)
    nav.tick(SCENE_PATH)
end
