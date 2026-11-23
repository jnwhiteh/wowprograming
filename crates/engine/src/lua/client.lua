-- Client functions written in Lua because they build on data that other
-- client functions return (named colors, for example).
local ipairs, pairs, type, rawget, rawset, setmetatable, _G = ipairs, pairs, type, rawget, rawset, setmetatable, _G

C_ColorOverrides = C_ColorOverrides or {}

-- Item quality index to the named color the client uses for it.
local qualityTags = {
    [0] = "ITEM_POOR_COLOR",
    [1] = "ITEM_STANDARD_COLOR",
    [2] = "ITEM_GOOD_COLOR",
    [3] = "ITEM_SUPERIOR_COLOR",
    [4] = "ITEM_EPIC_COLOR",
    [5] = "ITEM_LEGENDARY_COLOR",
    [6] = "ITEM_ARTIFACT_COLOR",
    [7] = "ITEM_WOW_TOKEN_COLOR", -- Heirloom
    [8] = "ITEM_WOW_TOKEN_COLOR",
}
-- Used when the named colors were not captured.
local qualityDefaults = {
    [0] = { 0.616, 0.616, 0.616 },
    [1] = { 1, 1, 1 },
    [2] = { 0.118, 1, 0 },
    [3] = { 0, 0.439, 0.867 },
    [4] = { 0.639, 0.208, 0.933 },
    [5] = { 1, 0.502, 0 },
    [6] = { 0.902, 0.8, 0.502 },
    [7] = { 0, 0.8, 1 },
    [8] = { 0, 0.8, 1 },
}

local function namedColor(tag)
    for _, c in ipairs(C_UIColor.GetColors()) do
        if c.baseTag == tag then return c.color end
    end
end

function C_ColorOverrides.GetColorForQuality(quality)
    local c = namedColor(qualityTags[quality] or "ITEM_STANDARD_COLOR")
    local r, g, b
    if c then
        r, g, b = c.r, c.g, c.b
    else
        local d = qualityDefaults[quality] or qualityDefaults[1]
        r, g, b = d[1], d[2], d[3]
    end
    local create = rawget(_G, "CreateColor")
    if create then return create(r, g, b, 1) end
    return { r = r, g = g, b = b, a = 1 }
end

function C_ColorOverrides.GetDefaultColorForQuality(quality)
    return C_ColorOverrides.GetColorForQuality(quality)
end

-- Enums that the game has but that are neither documented nor listed when
-- the capture script walks Enum. The UI only uses them as table keys at
-- load time, so made-up distinct values are enough until a capture records
-- the real ones (see tools/capture/client_constants_capture.lua).
local function placeholderEnum()
    local nextValue = 1000
    return setmetatable({}, {
        __index = function(t, k)
            nextValue = nextValue + 1
            rawset(t, k, nextValue)
            return nextValue
        end,
    })
end
for _, name in ipairs({ "StoreError", "VasServiceType", "VasPurchaseState", "VasQueueStatus",
        "PurchaseEligibility", "BattlepayBannerType", "BattlepayBoostProduct",
        "BattlepayCardType", "BattlepayDisplayFlag", "BattlepayGroupDisplayType",
        "BattlepayProductDecorator", "BattlepayProductGroupFlag", "ValueAddedServiceType" }) do
    if Enum and rawget(Enum, name) == nil then
        Enum[name] = placeholderEnum()
    end
end

-- The client runs some Blizzard addons (store, aura containers, restricted
-- frames) in a separate secure environment. The sandbox runs them in the
-- global environment, so these switches do nothing.
function GetGlobalEnvironment() return _G end
function GetCurrentEnvironment() return _G end
function SwapToGlobalEnvironment() end

-- Function containers: callable objects that wrap a Lua function and can
-- be cancelled. The client passes them to native callback registries.
C_FunctionContainers = C_FunctionContainers or {}
function C_FunctionContainers.CreateCallback(func)
    local container = {}
    local cancelled = false
    function container:Cancel() cancelled = true end
    function container:IsCancelled() return cancelled end
    function container:Invoke(...)
        if not cancelled then return func(...) end
    end
    return setmetatable(container, {
        __call = function(_, ...)
            if not cancelled then return func(...) end
        end,
    })
end

-- securecopy(value): a copy that secure code can trust. Without taint the
-- sandbox only needs the copy.
local function deepcopy(v, seen)
    if type(v) ~= "table" then return v end
    seen = seen or {}
    if seen[v] then return seen[v] end
    local out = {}
    seen[v] = out
    for k, val in pairs(v) do out[deepcopy(k, seen)] = deepcopy(val, seen) end
    return out
end
function securecopy(v) return deepcopy(v) end

-- The player always exists. Other units exist only if a scenario says so
-- (captured UnitExists answers replace this function for their units).
function UnitExists(unit)
    return unit == "player"
end

-- Edit mode layouts: without a capture, no saved layouts and the first
-- preset (Modern) active, as for a new character.
C_EditMode = C_EditMode or {}
function C_EditMode.GetLayouts()
    return { layouts = {}, activeLayout = 1 }
end

-- Units without an assigned group role report "NONE".
function UnitGroupRolesAssigned(unit)
    return "NONE"
end

-- SetPortraitTexture draws a unit's 3D model into a texture, which the
-- sandbox cannot do. Show the client's placeholder portrait instead.
function SetPortraitTexture(texture, unit)
    if texture and texture.SetTexture then
        texture:SetTexture("Interface\\CharacterFrame\\TempPortrait")
    end
end

-- Health and power of the player without a capture: full bars.
local function playerValue(value)
    return function(unit, ...)
        if unit == "player" then return value end
        return 0
    end
end
UnitHealth = playerValue(100)
UnitHealthMax = playerValue(100)
UnitPower = playerValue(100)
UnitPowerMax = playerValue(100)

-- Power type without a capture: mana.
function UnitPowerType(unit)
    return 0, "MANA", 0, 0, 0
end

-- Nobody offers a resurrection. (The generated stub returns "", which Lua
-- treats as true.)
function ResurrectGetOfferer() return nil end
function UnitGroupRolesAssignedEnum(unit)
    return nil
end

-- The action bar starts on page 1. (The generated stub returns 0.)
C_ActionBar = C_ActionBar or {}
function C_ActionBar.GetActionBarPage() return 1 end

-- GetBindingText(key, abbreviate): the display name of a key, from the
-- KEY_* global strings; modifiers become "s-", "c-" and "a-" when
-- abbreviated.
function GetBindingText(key, abbreviate)
    if type(key) ~= "string" or key == "" then return "" end
    local mods, base = key:match("^(.-)([^%-]+)$")
    if base == "" or base == nil then mods, base = "", key end
    local name = rawget(_G, "KEY_" .. base) or base
    if abbreviate then
        local short = rawget(_G, "KEY_" .. base .. "_ABBR")
        if type(short) == "string" then name = short end
        mods = mods:gsub("SHIFT%-", "s-"):gsub("CTRL%-", "c-"):gsub("ALT%-", "a-"):gsub("META%-", "m-")
    else
        mods = mods:gsub("SHIFT%-", "Shift-"):gsub("CTRL%-", "Ctrl-"):gsub("ALT%-", "Alt-")
    end
    return mods .. name
end

-- ModelScene view insets (the sandbox draws no models, but layout code
-- reads them).
local modelSceneMeta = rawget(_G, "GetModelSceneMetatable") and GetModelSceneMetatable()
if modelSceneMeta and type(modelSceneMeta.__index) == "table" then
    local insets = setmetatable({}, { __mode = "k" })
    modelSceneMeta.__index.SetViewInsets = function(self, left, right, top, bottom)
        insets[self] = { left or 0, right or 0, top or 0, bottom or 0 }
    end
    modelSceneMeta.__index.GetViewInsets = function(self)
        local v = insets[self]
        if not v then return 0, 0, 0, 0 end
        return v[1], v[2], v[3], v[4]
    end
end

-- Tooltip data without a capture: the spell's name and subtext (and its
-- description, if known). A capture records the client's full tooltips
-- (C_TooltipInfo.GetSpellBookItem and friends), which replace these.
C_TooltipInfo = C_TooltipInfo or {}
local function spellTooltip(spellID, name, subName)
    if not name then return nil end
    local white = CreateColor and CreateColor(1, 1, 1, 1)
    local gold = CreateColor and CreateColor(1, 0.82, 0, 1)
    local lines = { { type = 0, leftText = name, leftColor = white } }
    if subName and subName ~= "" then
        lines[1].rightText = subName
        lines[1].rightColor = CreateColor and CreateColor(0.5, 0.5, 0.5, 1)
    end
    local desc = spellID and C_Spell and C_Spell.GetSpellDescription and C_Spell.GetSpellDescription(spellID)
    if type(desc) == "string" and desc ~= "" then
        lines[#lines + 1] = { type = 0, leftText = desc, leftColor = gold, wrapText = true }
    end
    return { type = Enum and Enum.TooltipDataType and Enum.TooltipDataType.Spell or 1,
             id = spellID, lines = lines }
end
function C_TooltipInfo.GetSpellBookItem(slot, bank)
    local info = C_SpellBook and C_SpellBook.GetSpellBookItemInfo(slot, bank)
    if type(info) ~= "table" then return nil end
    return spellTooltip(info.spellID, info.name, info.subName)
end
function C_TooltipInfo.GetSpellByID(spellID)
    local info = C_Spell and C_Spell.GetSpellInfo(spellID)
    if type(info) ~= "table" then return nil end
    local sub = C_Spell.GetSpellSubtext and C_Spell.GetSpellSubtext(spellID)
    return spellTooltip(spellID, info.name, sub)
end

-- Secret values and protected objects: the sandbox has neither, so every
-- value is accessible and no value is secret.
function CheckAllowProtectedFunctions() return true end
function canaccessallvalues() return true end
function canaccessvalue() return true end
function canaccesstable() return true end
function canaccesssecrets() return true end
function issecretvalue() return false end
function issecrettable() return false end
function hasanysecretvalues() return false end
C_RestrictedActions = C_RestrictedActions or {}
function C_RestrictedActions.CheckAllowProtectedFunctions() return true end

-- mapvalues(func, ...): func applied to each argument.
function mapvalues(func, ...)
    local n = select("#", ...)
    local out = { ... }
    for i = 1, n do out[i] = func(out[i]) end
    return unpack(out, 1, n)
end

-- No mirror timers (breath, fatigue) run in the sandbox.
function GetMirrorTimerProgress() return 0 end
