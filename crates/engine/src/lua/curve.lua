-- C_CurveUtil: curve objects that map an x value to a y value.
-- Runs once at startup and returns the C_CurveUtil table.

local setmetatable, table, ipairs, type, error = setmetatable, table, ipairs, type, error

local LINEAR, STEP = 0, 1

local Curve = {}
Curve.__index = Curve

local function new()
    return setmetatable({ points = {}, curveType = LINEAR }, Curve)
end

function Curve:AddPoint(x, y)
    if type(x) ~= "number" or type(y) ~= "number" then
        error("Usage: curve:AddPoint(x, y)", 2)
    end
    local points = self.points
    local i = #points + 1
    while i > 1 and points[i - 1].x > x do
        i = i - 1
    end
    table.insert(points, i, { x = x, y = y })
end

function Curve:ClearPoints() self.points = {} end
function Curve:GetPointCount() return #self.points end

function Curve:GetPoint(index)
    local p = self.points[index]
    if p then return p.x, p.y end
end

function Curve:GetPoints()
    local out = {}
    for i, p in ipairs(self.points) do out[i] = { x = p.x, y = p.y } end
    return out
end

function Curve:SetPoints(points)
    self:ClearPoints()
    for _, p in ipairs(points) do self:AddPoint(p.x, p.y) end
end

function Curve:RemovePoint(index) table.remove(self.points, index) end
function Curve:GetType() return self.curveType end
function Curve:SetType(curveType) self.curveType = curveType end
function Curve:HasSecretValues() return false end

function Curve:SetToDefaults()
    self.points = {}
    self.curveType = LINEAR
end

function Curve:Copy()
    local copy = new()
    copy.curveType = self.curveType
    copy:SetPoints(self.points)
    return copy
end

function Curve:Evaluate(x)
    local points = self.points
    local n = #points
    if n == 0 then return 0 end
    if x <= points[1].x then return points[1].y end
    if x >= points[n].x then return points[n].y end
    for i = 2, n do
        local b = points[i]
        if x < b.x then
            local a = points[i - 1]
            if self.curveType == STEP then return a.y end
            return a.y + (b.y - a.y) * (x - a.x) / (b.x - a.x)
        end
    end
    return points[n].y
end

return {
    CreateCurve = new,
}
