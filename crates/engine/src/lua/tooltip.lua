-- Native GameTooltip methods. Tooltip contents are kept, not drawn yet.
-- Returns a table of methods for the GameTooltip widget type.
local setmetatable, select, tostring = setmetatable, select, tostring
local data = setmetatable({}, { __mode = "k" })

local function d(self)
    local t = data[self]
    if not t then
        t = { lines = {}, minWidth = 0, padding = { 0, 0, 0, 0 } }
        data[self] = t
    end
    return t
end

local M = {}
function M:SetOwner(owner, anchor, x, y)
    local t = d(self)
    t.owner, t.anchor, t.x, t.y = owner, anchor or "ANCHOR_LEFT", x or 0, y or 0
    t.lines = {}
end
function M:GetOwner() return d(self).owner end
function M:IsOwned(frame) return d(self).owner == frame end
function M:SetAnchorType(anchor, x, y) local t = d(self); t.anchor, t.x, t.y = anchor, x or 0, y or 0 end
function M:GetAnchorType() return d(self).anchor end
function M:ClearLines() d(self).lines = {} end
function M:AddLine(text) local l = d(self).lines; l[#l + 1] = { tostring(text or "") } end
function M:AddDoubleLine(a, b) local l = d(self).lines; l[#l + 1] = { tostring(a or ""), tostring(b or "") } end
function M:SetText(text) d(self).lines = { { tostring(text or "") } } end
function M:AppendText(text)
    local l = d(self).lines
    if l[1] then l[1][1] = l[1][1] .. tostring(text or "") else l[1] = { tostring(text or "") } end
end
function M:NumLines() return #d(self).lines end
function M:SetMinimumWidth(w) d(self).minWidth = w end
function M:GetMinimumWidth() return d(self).minWidth end
function M:SetPadding(r, b, l, t) d(self).padding = { r or 0, b or 0, l or 0, t or 0 } end
function M:GetPadding() local p = d(self).padding; return p[1], p[2], p[3], p[4] end
function M:FadeOut() self:Hide() end
function M:IsUnit() return false end
function M:GetUnit() return nil end
function M:GetSpell() return nil end
function M:GetItem() return nil end
function M:IsEquippedItem() return false end
function M:SetCustomLineSpacing() end
function M:SetAllowShowWithNoLines() end
function M:GetAllowShowWithNoLines() return false end
function M:GetTooltipData() return nil end
function M:CopyTooltip() end
function M:SetCustomWordWrapMinWidth() end
for _, name in ipairs({
    "SetSpellBookItem", "SetSpellByID", "SetHyperlink", "SetUnit", "SetItemByID",
    "SetAction", "SetInventoryItem", "SetBagItem", "SetTalent", "SetPvpTalent",
    "SetUnitBuff", "SetUnitDebuff", "SetUnitAura", "SetToyByItemID", "SetCurrencyByID",
}) do
    M[name] = function() end
end
return M
