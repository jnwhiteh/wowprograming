-- UnitPositionFrame methods: the client draws a pin for each added unit
-- at its position on the frame's map (the player arrow on the world map).
-- The sandbox draws each pin as a texture on the frame, placed from
-- C_Map.GetPlayerMapPosition and turned to the player's facing, and moves
-- the pins every frame. Units other than the player have no position yet.
local setmetatable, pairs, ipairs, type = setmetatable, pairs, ipairs, type

local M = {}
local data = setmetatable({}, { __mode = "k" }) -- frame -> { map, units, pins, ping }
local live = setmetatable({}, { __mode = "k" })

local function d(self)
    local t = data[self]
    if not t then
        t = { units = {}, pins = {}, pingScale = 1 }
        data[self] = t
    end
    return t
end

local function place(self)
    local t = d(self)
    local map = t.map
    for unit, u in pairs(t.units) do
        local pin = t.pins[unit]
        local pos = map and C_Map.GetPlayerMapPosition(map, unit)
        if pin and pos and self:IsVisible() then
            local x, y = pos:GetXY()
            local w, h = self:GetSize()
            -- Pin sizes are screen sizes; the frame sits on a scaled canvas.
            local scale = self:GetEffectiveScale() / UIParent:GetEffectiveScale()
            pin:SetSize(u.width / scale, u.height / scale)
            pin:ClearAllPoints()
            pin:SetPoint("CENTER", self, "TOPLEFT", x * w, -y * h)
            if u.showRotation and unit == "player" then
                pin:SetRotation(GetPlayerFacing() or 0)
            end
            pin:Show()
        elseif pin then
            pin:Hide()
        end
    end
end

local driver
local function ensureDriver()
    if driver then return end
    driver = CreateFrame("Frame")
    driver:SetScript("OnUpdate", function()
        for frame in pairs(live) do place(frame) end
    end)
end

function M:AddUnit(unit, atlas, width, height, r, g, b, a, sublevel, showRotation)
    local t = d(self)
    t.units[unit] = { atlas = atlas, width = width or 16, height = height or 16,
                      color = { r or 1, g or 1, b or 1, a or 1 }, sublevel = sublevel or 0,
                      showRotation = showRotation }
end

function M:ClearUnits()
    local t = d(self)
    for _, pin in pairs(t.pins) do pin:Hide() end
    t.units = {}
end

function M:FinalizeUnits()
    local t = d(self)
    for unit, u in pairs(t.units) do
        local pin = t.pins[unit]
        if not pin then
            pin = self:CreateTexture(nil, "OVERLAY", nil, math.min(7, math.max(-8, u.sublevel)))
            t.pins[unit] = pin
        end
        if C_Texture.GetAtlasInfo(u.atlas) then
            pin:SetAtlas(u.atlas)
        elseif u.atlas == "UI-WorldMapArrow" then
            -- Not an atlas in the game data: the client's own arrow
            -- texture (inferred).
            pin:SetTexture("Interface\\WorldMap\\WorldMapArrow")
        else
            pin:SetTexture(u.atlas)
        end
        pin:SetVertexColor(u.color[1], u.color[2], u.color[3], u.color[4])
    end
    live[self] = true
    ensureDriver()
    place(self)
end

function M:SetUnitColor(unit, r, g, b, a)
    local t = d(self)
    local u = t.units[unit]
    if u then u.color = { r, g, b, a or 1 } end
    local pin = t.pins[unit]
    if pin then pin:SetVertexColor(r, g, b, a or 1) end
end

function M:SetUiMapID(id) d(self).map = id end
function M:GetUiMapID() return d(self).map end
function M:GetMouseOverUnits() end
function M:SetPlayerPingTexture() end
function M:SetPlayerPingScale(scale) d(self).pingScale = scale end
function M:GetPlayerPingScale() return d(self).pingScale end
function M:StartPlayerPing() end
function M:StopPlayerPing() end

return M
