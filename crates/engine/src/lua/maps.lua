-- Maps, the player's position and movement, as the C++ client provides
-- them.
--
-- Map data comes from the game's UiMap tables (maps.rs): each map's name,
-- parent, type and, when it has world positions, its continent and the
-- world box it covers (left, right, top, bottom). World coordinates are
-- the client's: x points north, y points west. A map position (0 to 1)
-- runs left to right along falling world y and top to bottom along
-- falling world x.
--
-- The player stands at a world position on a continent and faces a
-- direction (radians, 0 is north, turning left adds). The movement keys
-- call MoveForwardStart and the other functions from Bindings_Standard;
-- the position changes over time at the client's base speeds. There is
-- no terrain, collision, falling or flying. Rules marked "inferred" in
-- docs/client-runtime.md.
local mapData, fire = ...
local type, pairs, ipairs, math, table, tostring = type, pairs, ipairs, math, table, tostring

local World = {}
WowProgrammingWorld = World

local RUN_SPEED, BACK_SPEED, TURN_SPEED = 7, 4.5, math.pi -- yards/s, yards/s, rad/s
local COSMIC = 946

local data -- { maps = { [id] = {...} }, groups = { ... } }, read on first use
local function maps()
    if data == nil then data = mapData() or false end
    return data and data.maps or {}
end
local function groups()
    maps()
    return data and data.groups or {}
end

local function details(id, m)
    return { mapID = id, name = m.name, mapType = m.type, parentMapID = m.parent, flags = m.flags }
end

local function vector(x, y)
    if CreateVector2D then return CreateVector2D(x, y) end
    return { x = x, y = y, GetXY = function(self) return self.x, self.y end }
end

local function hasRect(m) return m and m.left ~= nil and m.left ~= m.right and m.top ~= m.bottom end

-- Map position <-> world position on the map's continent.
local function toWorld(m, x, y)
    return m.top + y * (m.bottom - m.top), m.left + x * (m.right - m.left)
end
local function toMap(m, wx, wy)
    return (wy - m.left) / (m.right - m.left), (wx - m.top) / (m.bottom - m.top)
end

local function contains(m, continent, wx, wy)
    if not hasRect(m) or m.continent ~= continent then return false end
    local x, y = toMap(m, wx, wy)
    return x >= 0 and x <= 1 and y >= 0 and y <= 1
end

-- The map the client shows for a world position: the smallest zone,
-- dungeon or micro map that contains it, else the smallest map of any
-- type (inferred; the client also uses the area the player stands in).
local function bestMapAt(continent, wx, wy)
    local best, bestArea, bestRank
    for id, m in pairs(maps()) do
        if m.system == 0 and contains(m, continent, wx, wy) then
            local rank = (m.type == 3 or m.type == 4 or m.type == 5) and 1 or 2
            local area = math.abs((m.right - m.left) * (m.bottom - m.top))
            if not best or rank < bestRank or (rank == bestRank and area < bestArea) then
                best, bestArea, bestRank = id, area, rank
            end
        end
    end
    return best
end

-- C_Map ---------------------------------------------------------------------------

C_Map = C_Map or {}

function C_Map.GetMapInfo(id)
    local m = maps()[id]
    return m and details(id, m) or nil
end

function C_Map.GetMapChildrenInfo(id, mapType, allDescendants)
    local out = {}
    local function walk(parent)
        for cid, m in pairs(maps()) do
            if m.parent == parent and cid ~= parent then
                if mapType == nil or m.type == mapType then out[#out + 1] = details(cid, m) end
                if allDescendants then walk(cid) end
            end
        end
    end
    walk(id)
    table.sort(out, function(a, b) return a.mapID < b.mapID end)
    return out
end

-- The first ancestor of mapType (the top-most one with topMost).
function C_Map.GetMapParentInfo(id, mapType, topMost)
    local found
    local m = maps()[id]
    while m and m.parent and m.parent ~= 0 do
        local pid = m.parent
        m = maps()[pid]
        if m and (mapType == nil or m.type == mapType) then
            found = details(pid, m)
            if not topMost then return found end
        end
    end
    return found
end

function C_Map.GetMapGroupID(id)
    for _, g in ipairs(groups()) do
        if g.map == id then return g.group end
    end
    return nil
end

function C_Map.GetMapGroupMembersInfo(group)
    local out = {}
    for _, g in ipairs(groups()) do
        if g.group == group then
            out[#out + 1] = { mapID = g.map, relativeHeightIndex = g.relativeHeightIndex, name = g.name }
        end
    end
    table.sort(out, function(a, b) return a.relativeHeightIndex < b.relativeHeightIndex end)
    return out
end

function C_Map.GetMapWorldSize(id)
    local m = maps()[id]
    if not hasRect(m) then return 0, 0 end
    return math.abs(m.left - m.right), math.abs(m.top - m.bottom)
end

function C_Map.GetWorldPosFromMapPos(id, pos)
    local m = maps()[id]
    if not hasRect(m) or type(pos) ~= "table" then return nil end
    local x, y = pos:GetXY()
    local wx, wy = toWorld(m, x, y)
    return m.continent, vector(wx, wy)
end

function C_Map.GetMapPosFromWorldPos(continent, pos, overrideMapID)
    if type(pos) ~= "table" then return nil end
    local wx, wy = pos:GetXY()
    local id = overrideMapID or bestMapAt(continent, wx, wy)
    local m = id and maps()[id]
    if not hasRect(m) then return nil end
    return id, vector(toMap(m, wx, wy))
end

-- Where a map lies on another map: minX, maxX, minY, maxY in the other
-- map's coordinates.
function C_Map.GetMapRectOnMap(id, topID)
    local m, top = maps()[id], maps()[topID]
    if not hasRect(m) or not hasRect(top) then return 0, 0, 0, 0 end
    local x1, y1 = toMap(top, m.top, m.left)
    local x2, y2 = toMap(top, m.bottom, m.right)
    return math.min(x1, x2), math.max(x1, x2), math.min(y1, y2), math.max(y1, y2)
end

-- The child map at a position on a map.
function C_Map.GetMapInfoAtPosition(id, x, y)
    local m = maps()[id]
    if not hasRect(m) then return C_Map.GetMapInfo(id) end
    local wx, wy = toWorld(m, x, y)
    local best, bestArea
    for cid, c in pairs(maps()) do
        if c.parent == id and contains(c, m.continent, wx, wy) then
            local area = math.abs((c.right - c.left) * (c.bottom - c.top))
            if not best or area < bestArea then best, bestArea = cid, area end
        end
    end
    return C_Map.GetMapInfo(best or id)
end

function C_Map.GetFallbackWorldMapID() return COSMIC end
function C_Map.MapHasArt(id) return maps()[id] ~= nil end
function C_Map.IsMapValidForNavBarDropdown(id) return maps()[id] ~= nil end
function C_Map.CanSetUserWaypointOnMap(id) return hasRect(maps()[id]) end

-- The player ------------------------------------------------------------------------

local player = { continent = nil, x = 0, y = 0, z = 0, facing = 0 }
local move = { forward = false, backward = false, left = false, right = false,
               turnLeft = false, turnRight = false, autorun = false }
local moving, turning = false, false
local currentMap

function C_Map.GetBestMapForUnit(unit)
    if unit ~= "player" or not player.continent then return nil end
    return currentMap or bestMapAt(player.continent, player.x, player.y)
end

function C_Map.GetPlayerMapPosition(id, unit)
    if unit ~= "player" or not player.continent then return nil end
    local m = maps()[id]
    if not hasRect(m) or m.continent ~= player.continent then return nil end
    return vector(toMap(m, player.x, player.y))
end

local previousUnitPosition = UnitPosition
function UnitPosition(unit)
    if (unit == "player" or (UnitIsUnit and UnitIsUnit(unit, "player"))) and player.continent then
        return player.x, player.y, player.z, player.continent
    end
    if previousUnitPosition then return previousUnitPosition(unit) end
    return nil
end

function GetPlayerFacing() return player.continent and player.facing or nil end

local function speed()
    if move.forward or move.autorun or move.left or move.right then return RUN_SPEED end
    if move.backward then return BACK_SPEED end
    return 0
end

function IsPlayerMoving() return moving end
function GetUnitSpeed(unit)
    if unit ~= "player" then return 0, RUN_SPEED, 0, 0 end
    return moving and speed() or 0, RUN_SPEED, 0, 4.72
end
function IsFalling() return false end
function IsFlying() return false end
function IsSwimming() return false end
function IsMounted() return false end
function IsIndoors() return false end
function IsOutdoors() return true end
function IsResting() return false end

local function zoneName()
    local m = currentMap and maps()[currentMap]
    if not m then return "" end
    -- The zone is the nearest zone (type 3) up the parents.
    local id, z = currentMap, m
    while z and z.type > 3 and z.parent and maps()[z.parent] do
        id = z.parent
        z = maps()[id]
    end
    return z and z.name or m.name
end
function GetZoneText() return zoneName() end
function GetRealZoneText() return zoneName() end
function GetSubZoneText()
    local m = currentMap and maps()[currentMap]
    if m and m.type > 3 then return m.name end
    return ""
end
function GetMinimapZoneText()
    local sub = GetSubZoneText()
    return sub ~= "" and sub or zoneName()
end
function GetZonePVPInfo() return nil end

local function updateMap(notify)
    if not player.continent then return end
    local id = bestMapAt(player.continent, player.x, player.y)
    if id == currentMap then return end
    local oldZone = zoneName()
    currentMap = id
    if notify then
        fire("ZONE_CHANGED")
        if zoneName() ~= oldZone then fire("ZONE_CHANGED_NEW_AREA") end
    end
end

-- Movement functions (called by the bindings in Bindings_Standard.xml).
local function setMove(key, value) return function() move[key] = value end end
MoveForwardStart, MoveForwardStop = setMove("forward", true), setMove("forward", false)
MoveBackwardStart, MoveBackwardStop = setMove("backward", true), setMove("backward", false)
StrafeLeftStart, StrafeLeftStop = setMove("left", true), setMove("left", false)
StrafeRightStart, StrafeRightStop = setMove("right", true), setMove("right", false)
TurnLeftStart, TurnLeftStop = setMove("turnLeft", true), setMove("turnLeft", false)
TurnRightStart, TurnRightStop = setMove("turnRight", true), setMove("turnRight", false)
function ToggleAutoRun() move.autorun = not move.autorun end
function StartAutoRun() move.autorun = true end
function StopAutoRun() move.autorun = false end
function JumpOrAscendStart() end
function AscendStop() end
function MoveAndSteerStart() move.forward = true end
function MoveAndSteerStop() move.forward = false end

-- A dead player cannot move (the client state model freezes it).
local frozen = false
local function step(elapsed)
    if not player.continent or frozen then return end
    local turn = (move.turnLeft and 1 or 0) - (move.turnRight and 1 or 0)
    local nowTurning = turn ~= 0
    if nowTurning then
        player.facing = (player.facing + turn * TURN_SPEED * elapsed) % (2 * math.pi)
    end
    if nowTurning ~= turning then
        turning = nowTurning
        fire(turning and "PLAYER_STARTED_TURNING" or "PLAYER_STOPPED_TURNING")
    end
    -- Forward and backward cancel; strafing adds a sideways part. Moving
    -- backward is slower (inferred: also when strafing back).
    local fwd = ((move.forward or move.autorun) and 1 or 0) - (move.backward and 1 or 0)
    local side = (move.left and 1 or 0) - (move.right and 1 or 0)
    local nowMoving = fwd ~= 0 or side ~= 0
    if nowMoving then
        local angle = player.facing + math.atan2(side, fwd)
        local v = fwd < 0 and BACK_SPEED or RUN_SPEED
        player.x = player.x + math.cos(angle) * v * elapsed
        player.y = player.y + math.sin(angle) * v * elapsed
        updateMap(true)
    end
    if nowMoving ~= moving then
        moving = nowMoving
        fire(moving and "PLAYER_STARTED_MOVING" or "PLAYER_STOPPED_MOVING")
    end
end

-- Scenario and test functions ----------------------------------------------------

-- Put the player on a map at a map position (0 to 1), facing a direction
-- (radians, default north). Returns false if the map has no world box.
function World.SetPlayerMapPosition(id, x, y, facing)
    local m = maps()[id]
    if not hasRect(m) then return false end
    player.continent = m.continent
    player.x, player.y = toWorld(m, x, y)
    player.facing = facing or 0
    currentMap = nil
    updateMap(false)
    return true
end

function World.SetPlayerWorldPosition(continent, x, y, facing)
    player.continent, player.x, player.y = continent, x, y
    player.facing = facing or player.facing
    currentMap = nil
    updateMap(false)
end

function World.Player() return player end
function World.SetFrozen(on) frozen = on and true or false end
function World.Step(elapsed) step(elapsed) end

local driver = CreateFrame("Frame")
driver:SetScript("OnUpdate", function(_, elapsed) step(elapsed) end)

return World
