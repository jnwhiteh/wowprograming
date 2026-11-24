-- Native GameTooltip methods. Called with the ScriptRegion methods; returns
-- the methods of the GameTooltip widget type.
--
-- Like the client, the tooltip creates its lines as font strings named
-- <Tooltip>TextLeft<n> and <Tooltip>TextRight<n>, sizes itself to them on
-- Show and anchors itself to its owner. Content comes from Blizzard's Lua
-- (TooltipDataHandlerMixin) through C_TooltipInfo, or from AddLine.
local FrameMethods, fireTooltipCleared = ...
local setmetatable, select, tostring, type, math, rawget, _G =
      setmetatable, select, tostring, type, math, rawget, _G

local PADDING = 10       -- left, right, top and bottom
local LINE_SPACING = 2
local RIGHT_GAP = 15     -- between left and right text of a double line
local WRAP_WIDTH = 250   -- width wrapped lines use when nothing is wider

local data = setmetatable({}, { __mode = "k" })

local function d(self)
    local t = data[self]
    if not t then
        t = { lines = {}, minWidth = 0, padding = { 0, 0, 0, 0 }, anchor = "ANCHOR_NONE",
              x = 0, y = 0, numLines = 0 }
        data[self] = t
    end
    return t
end

local function fontObject(name)
    local f = rawget(_G, name)
    if type(f) == "table" then return f end
end

-- The font strings of line n, created on first use.
local function lineStrings(self, n)
    local t = d(self)
    local strings = t.strings
    if not strings then
        strings = {}
        t.strings = strings
    end
    local s = strings[n]
    if s then return s[1], s[2] end
    local name = self:GetName()
    -- Templates define the first lines (TextLeft1, TextRight1, ...); the
    -- tooltip creates the rest.
    local left = self["TextLeft" .. n] or (name and rawget(_G, name .. "TextLeft" .. n))
    local right = self["TextRight" .. n] or (name and rawget(_G, name .. "TextRight" .. n))
    if left and right then
        strings[n] = { left, right }
        return left, right
    end
    left = self:CreateFontString(name and (name .. "TextLeft" .. n) or nil, "ARTWORK")
    right = self:CreateFontString(name and (name .. "TextRight" .. n) or nil, "ARTWORK")
    self["TextLeft" .. n], self["TextRight" .. n] = left, right
    local leftFont = n == 1 and (self.textLeft1Font or "GameTooltipHeaderText")
        or (self.textLeft2Font or "GameTooltipText")
    local rightFont = n == 1 and (self.textRight1Font or "GameTooltipHeaderText")
        or (self.textRight2Font or "GameTooltipText")
    local lf, rf = fontObject(leftFont), fontObject(rightFont)
    if lf then left:SetFontObject(lf) end
    if rf then right:SetFontObject(rf) end
    left:SetJustifyH("LEFT")
    right:SetJustifyH("RIGHT")
    left:Hide()
    right:Hide()
    strings[n] = { left, right }
    return left, right
end

local function setColor(fs, r, g, b, a, default)
    if r == nil then
        r, g, b = default[1], default[2], default[3]
    end
    fs:SetTextColor(r, g or 1, b or 1, a or 1)
end

local WHITE = { 1, 1, 1 }
local GOLD = { 1, 0.82, 0 }

local function addLine(self, left, right, lr, lg, lb, rr, rg, rb, wrap, default)
    local t = d(self)
    local n = t.numLines + 1
    t.numLines = n
    local l, r = lineStrings(self, n)
    l:SetText(left)
    setColor(l, lr, lg, lb, nil, default)
    l:Show()
    t.lines[n] = { wrap = wrap and true or false }
    if right then
        r:SetText(right)
        setColor(r, rr, rg, rb, nil, default)
        r:Show()
    else
        r:SetText("")
        r:Hide()
    end
end

local M = {}

function M:SetOwner(owner, anchor, x, y)
    local t = d(self)
    t.owner, t.anchor, t.x, t.y = owner, anchor or "ANCHOR_LEFT", x or 0, y or 0
    M.ClearLines(self)
    self:Hide()
end
function M:GetOwner() return d(self).owner end
function M:IsOwned(frame) return d(self).owner == frame end
function M:SetAnchorType(anchor, x, y) local t = d(self); t.anchor, t.x, t.y = anchor, x or 0, y or 0 end
function M:GetAnchorType() local t = d(self); return t.anchor, t.x, t.y end

function M:ClearLines()
    local t = d(self)
    for _, s in ipairs(t.strings or {}) do
        s[1]:SetText("")
        s[1]:Hide()
        s[2]:SetText("")
        s[2]:Hide()
    end
    t.numLines = 0
    t.lines = {}
    -- The handler and its hooks; script errors are reported without
    -- stopping the tooltip, as in the client.
    fireTooltipCleared(self)
end

function M:AddLine(text, r, g, b, wrap)
    addLine(self, tostring(text or ""), nil, r, g, b, nil, nil, nil, wrap, GOLD)
end
function M:AddDoubleLine(left, right, lr, lg, lb, rr, rg, rb)
    addLine(self, tostring(left or ""), tostring(right or ""), lr, lg, lb, rr, rg, rb, false, GOLD)
end
function M:SetText(text, r, g, b, a, wrap)
    M.ClearLines(self)
    addLine(self, tostring(text or ""), nil, r, g, b, nil, nil, nil, wrap, WHITE)
end
function M:AppendText(text)
    local t = d(self)
    if t.numLines == 0 then
        return M.SetText(self, text)
    end
    local l = lineStrings(self, 1)
    l:SetText((l:GetText() or "") .. tostring(text or ""))
end
function M:NumLines() return d(self).numLines end
function M:SetMinimumWidth(w) d(self).minWidth = w or 0 end
function M:GetMinimumWidth() return d(self).minWidth end
function M:SetPadding(r, b, l, t) d(self).padding = { r or 0, b or 0, l or 0, t or 0 } end
function M:GetPadding() local p = d(self).padding; return p[1], p[2], p[3], p[4] end
function M:SetCustomWordWrapMinWidth(w) d(self).wrapWidth = w end

-- Size the tooltip to its lines and place it relative to its owner.
local function layout(self)
    local t = d(self)
    local n = t.numLines
    local pad = t.padding
    local widest = 0
    -- First pass: widest line that does not wrap.
    for i = 1, n do
        local l, r = lineStrings(self, i)
        l:ClearAllPoints()
        r:ClearAllPoints()
        l:SetWidth(0)
        if not t.lines[i].wrap then
            local w = l:GetUnboundedStringWidth()
            if r:IsShown() then w = w + RIGHT_GAP + r:GetUnboundedStringWidth() end
            widest = math.max(widest, w)
        end
    end
    local wrapWidth = math.max(widest, t.wrapWidth or WRAP_WIDTH)
    local height = 0
    for i = 1, n do
        local l, r = lineStrings(self, i)
        if t.lines[i].wrap then
            local w = l:GetUnboundedStringWidth()
            if w > wrapWidth then
                l:SetWidth(wrapWidth)
                widest = math.max(widest, wrapWidth)
            else
                widest = math.max(widest, w)
            end
        end
        if i == 1 then
            l:SetPoint("TOPLEFT", self, "TOPLEFT", PADDING + pad[3], -PADDING - pad[4])
        else
            local prev = lineStrings(self, i - 1)
            l:SetPoint("TOPLEFT", prev, "BOTTOMLEFT", 0, -LINE_SPACING)
        end
        r:SetPoint("TOP", l, "TOP")
        r:SetPoint("RIGHT", self, "RIGHT", -PADDING - pad[1], 0)
        height = height + l:GetStringHeight() + (i > 1 and LINE_SPACING or 0)
    end
    local width = math.max(widest + 2 * PADDING + pad[1] + pad[3], t.minWidth or 0)
    self:SetSize(width, height + 2 * PADDING + pad[2] + pad[4])

    local owner, anchor = t.owner, t.anchor or "ANCHOR_NONE"
    if owner and anchor ~= "ANCHOR_NONE" and anchor ~= "ANCHOR_PRESERVE" then
        self:ClearAllPoints()
        local x, y = t.x or 0, t.y or 0
        if anchor == "ANCHOR_RIGHT" then
            self:SetPoint("BOTTOMLEFT", owner, "TOPRIGHT", x, y)
        elseif anchor == "ANCHOR_LEFT" then
            self:SetPoint("BOTTOMRIGHT", owner, "TOPLEFT", x, y)
        elseif anchor == "ANCHOR_TOP" then
            self:SetPoint("BOTTOM", owner, "TOP", x, y)
        elseif anchor == "ANCHOR_BOTTOM" then
            self:SetPoint("TOP", owner, "BOTTOM", x, y)
        elseif anchor == "ANCHOR_TOPLEFT" then
            self:SetPoint("BOTTOMLEFT", owner, "TOPLEFT", x, y)
        elseif anchor == "ANCHOR_TOPRIGHT" then
            self:SetPoint("BOTTOMRIGHT", owner, "TOPRIGHT", x, y)
        elseif anchor == "ANCHOR_BOTTOMLEFT" then
            self:SetPoint("TOPRIGHT", owner, "BOTTOMLEFT", x, y)
        elseif anchor == "ANCHOR_BOTTOMRIGHT" then
            self:SetPoint("TOPLEFT", owner, "BOTTOMRIGHT", x, y)
        elseif anchor == "ANCHOR_CURSOR" or anchor == "ANCHOR_CURSOR_RIGHT" then
            local cx, cy = GetCursorPosition()
            local s = UIParent:GetEffectiveScale()
            self:SetPoint("BOTTOMLEFT", UIParent, "BOTTOMLEFT", cx / s + x, cy / s + y)
        else
            self:SetPoint("BOTTOMLEFT", owner, "TOPRIGHT", x, y)
        end
    elseif owner == nil and anchor ~= "ANCHOR_PRESERVE" and self:GetNumPoints() == 0 then
        local onDefault = self:GetScript("OnTooltipSetDefaultAnchor")
        if onDefault then securecall(onDefault, self) end
    end
end

function M:Show()
    -- A tooltip without lines stays hidden unless allowed.
    local t = d(self)
    if t.numLines == 0 and not t.allowNoLines then
        return self:Hide()
    end
    layout(self)
    return FrameMethods.Show(self)
end

function M:FadeOut() self:Hide() end
function M:IsUnit() return false end
function M:SetCustomLineSpacing() end
function M:SetAllowShowWithNoLines(allow) d(self).allowNoLines = allow and true or false end
function M:GetAllowShowWithNoLines() return d(self).allowNoLines == true end
function M:CopyTooltip() end

return M
