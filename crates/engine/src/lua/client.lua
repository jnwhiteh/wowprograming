-- Client functions written in Lua because they build on data that other
-- client functions return (named colors, for example).
local ipairs, pairs, type, rawget, rawset, setmetatable, _G = ipairs, pairs, type, rawget, rawset, setmetatable, _G
local fireNow = ...

-- Events that answer a request arrive on a later frame, as in the client.
local function fireLater(event, ...)
    local args, n = { ... }, select("#", ...)
    C_Timer.After(0, function() fireNow(event, unpack(args, 1, n)) end)
end

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

-- Class and race of a unit that does not exist: nothing, as in the
-- client (the generated stubs return "", which Blizzard code takes for a
-- class name). Existing units without other data keep the stub answers.
function UnitClass(unit)
    if UnitExists(unit) then return "", "", 0 end
end
function UnitClassBase(unit)
    if UnitExists(unit) then return "", 0 end
end
function UnitRace(unit)
    if UnitExists(unit) then return "", "", 0 end
end

-- Edit mode: the client keeps the saved layouts and account settings.
-- The sandbox stores them in two SavedVariables of its own (see
-- docs/client-runtime.md, "Edit Mode"): account layouts and settings in
-- WowProgramming_EditModeAccount, character layouts and the active
-- layout in WowProgramming_EditModeCharacter. A new character has no
-- saved layouts and the first preset (Modern) active.
C_EditMode = C_EditMode or {}
local function editModeStore()
    local account = rawget(_G, "WowProgramming_EditModeAccount")
    if type(account) ~= "table" then
        account = { layouts = {}, settings = {} }
        rawset(_G, "WowProgramming_EditModeAccount", account)
    end
    local character = rawget(_G, "WowProgramming_EditModeCharacter")
    if type(character) ~= "table" then
        character = { layouts = {}, activeLayout = 1 }
        rawset(_G, "WowProgramming_EditModeCharacter", character)
    end
    return account, character
end
local LAYOUT_PRESET, LAYOUT_ACCOUNT = 0, 1
function C_EditMode.GetLayouts()
    local account, character = editModeStore()
    local layouts = {}
    for _, l in ipairs(account.layouts) do layouts[#layouts + 1] = deepcopy(l) end
    for _, l in ipairs(character.layouts) do layouts[#layouts + 1] = deepcopy(l) end
    return { layouts = layouts, activeLayout = character.activeLayout }
end
function C_EditMode.SaveLayouts(saveInfo)
    local account, character = editModeStore()
    account.layouts, character.layouts = {}, {}
    for _, l in ipairs(saveInfo.layouts or {}) do
        -- Presets come from Blizzard's Lua; only the player's own layouts
        -- are stored.
        if l.layoutType == LAYOUT_ACCOUNT then
            account.layouts[#account.layouts + 1] = deepcopy(l)
        elseif l.layoutType ~= LAYOUT_PRESET then
            character.layouts[#character.layouts + 1] = deepcopy(l)
        end
    end
    character.activeLayout = saveInfo.activeLayout or character.activeLayout
end
-- Changing the active layout sends the layouts again; Blizzard's Edit
-- Mode applies the active one when EDIT_MODE_LAYOUTS_UPDATED arrives
-- (inferred from SelectLayout, which does nothing else).
function C_EditMode.SetActiveLayout(index)
    local _, character = editModeStore()
    character.activeLayout = index
    fireLater("EDIT_MODE_LAYOUTS_UPDATED", C_EditMode.GetLayouts(), false)
end
function C_EditMode.GetAccountSettings()
    local account = editModeStore()
    local out = {}
    for _, setting in pairs(Enum.EditModeAccountSetting or {}) do
        local default = 0
        if setting == (Enum.EditModeAccountSetting.GridSpacing) then
            local c = Constants.EditModeConsts
            default = c and c.EditModeDefaultGridSpacing or 100
        end
        out[#out + 1] = { setting = setting, value = account.settings[setting] or default }
    end
    table.sort(out, function(a, b) return a.setting < b.setting end)
    return out
end
function C_EditMode.SetAccountSetting(setting, value)
    local account = editModeStore()
    account.settings[setting] = value
end
function C_EditMode.IsValidLayoutName(name)
    return type(name) == "string" and name:match("%S") ~= nil
end
function C_EditMode.OnEditModeExit() end
function C_EditMode.OnLayoutAdded(index, activate)
    if activate then
        C_EditMode.SetActiveLayout(index)
    end
end
-- After a deletion the client activates a preset (inferred: the first).
function C_EditMode.OnLayoutDeleted(index)
    local _, character = editModeStore()
    if character.activeLayout == index then
        C_EditMode.SetActiveLayout(1)
    elseif character.activeLayout > index then
        C_EditMode.SetActiveLayout(character.activeLayout - 1)
    end
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
UnitHealthMax = playerValue(100)

-- Class resources asked for by power type (Enum.PowerType), when they are
-- not the unit's main power: the usual maximum (inferred; talents can
-- raise some). Combo points, holy power, chi and arcane charges start
-- empty; runes and essence start full; soul shards rest at 3.
local RESOURCE_MAX = { [4] = 5, [5] = 6, [7] = 5, [9] = 5, [12] = 5, [16] = 4, [19] = 5 }
local RESOURCE_AT_REST = { [5] = 6, [7] = 3, [19] = 5 }
local function isOtherPower(unit, powerType)
    return powerType ~= nil and powerType ~= (UnitPowerType(unit))
end
function UnitPowerMax(unit, powerType)
    if unit ~= "player" then return 0 end
    if isOtherPower(unit, powerType) then return RESOURCE_MAX[powerType] or 0 end
    return 100
end
-- Current health and power follow the maximum, looked up at call time: a
-- capture replays the maximum, but not the current value, which is secret
-- in the game. (A low value shows the flashing low-health warning.)
function UnitHealth(unit, ...)
    return UnitHealthMax(unit, ...)
end
function UnitPower(unit, powerType, ...)
    if isOtherPower(unit, powerType) then
        if not UnitExists(unit) then return 0 end
        return RESOURCE_AT_REST[powerType] or 0
    end
    return UnitPowerMax(unit, powerType, ...)
end

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

-- Aura tooltips: the aura's spell name and description, and the time left.
local function auraTooltip(aura)
    if type(aura) ~= "table" or not aura.name then return nil end
    local data = spellTooltip(aura.spellId, aura.name)
    data.type = Enum and Enum.TooltipDataType and Enum.TooltipDataType.UnitAura or data.type
    local left = (aura.expirationTime or 0) - GetTime()
    if (aura.duration or 0) > 0 and left > 0 and SecondsToTime then
        local gold = CreateColor and CreateColor(1, 0.82, 0, 1)
        data.lines[#data.lines + 1] = { type = 0, leftText = SecondsToTime(left) .. " remaining",
                                        leftColor = gold }
    end
    return data
end
local function auraByIndex(unit, index, filter)
    return C_UnitAuras and C_UnitAuras.GetAuraDataByIndex
        and C_UnitAuras.GetAuraDataByIndex(unit, index, filter)
end
local function auraByID(unit, id)
    return C_UnitAuras and C_UnitAuras.GetAuraDataByAuraInstanceID
        and C_UnitAuras.GetAuraDataByAuraInstanceID(unit, id)
end
function C_TooltipInfo.GetUnitAura(unit, index, filter)
    return auraTooltip(auraByIndex(unit, index, filter))
end
function C_TooltipInfo.GetUnitBuff(unit, index, filter)
    return auraTooltip(auraByIndex(unit, index, filter or "HELPFUL"))
end
function C_TooltipInfo.GetUnitDebuff(unit, index, filter)
    return auraTooltip(auraByIndex(unit, index, filter or "HARMFUL"))
end
function C_TooltipInfo.GetUnitAuraByAuraInstanceID(unit, id)
    return auraTooltip(auraByID(unit, id))
end
C_TooltipInfo.GetUnitBuffByAuraInstanceID = C_TooltipInfo.GetUnitAuraByAuraInstanceID
C_TooltipInfo.GetUnitDebuffByAuraInstanceID = C_TooltipInfo.GetUnitAuraByAuraInstanceID

-- Spell book item type and texture come from the item's info (captures
-- record GetSpellBookItemInfo; these two often are not recorded).
C_SpellBook = C_SpellBook or {}
local function spellBookItem(slot, bank)
    local info = C_SpellBook.GetSpellBookItemInfo and C_SpellBook.GetSpellBookItemInfo(slot, bank)
    if type(info) == "table" then return info end
end
function C_SpellBook.GetSpellBookItemType(slot, bank)
    local info = spellBookItem(slot, bank)
    if not info then return Enum and Enum.SpellBookItemType and Enum.SpellBookItemType.None or 0, 0 end
    return info.itemType, info.actionID, info.spellID
end
function C_SpellBook.GetSpellBookItemTexture(slot, bank)
    local info = spellBookItem(slot, bank)
    return info and info.iconID or 0
end

-- A spell exists when the client has a record for it.
-- Spell:ContinueWithCancelOnSpellLoad needs this.
C_Spell = C_Spell or {}
function C_Spell.DoesSpellExist(spell)
    local getInfo = C_Spell.GetSpellInfo
    return spell ~= nil and getInfo ~= nil and type(getInfo(spell)) == "table"
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
-- Wrapping, unwrapping and scrubbing return the values unchanged. (The
-- generated stubs return nothing, which loses the values: the target
-- frame's aura row count, for example.)
function secretwrap(...) return ... end
function secretunwrap(...) return ... end
function scrubsecretvalues(...) return ... end
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

-- SimpleHTML font methods take an optional element first ("P", "H1",
-- "H2", "H3"). The sandbox has one font per widget: apply them to it.
do
    local mt = GetSimpleHTMLMetatable and GetSimpleHTMLMetatable()
    local methods = mt and type(mt.__index) == "table" and mt.__index
    local elements = { P = true, H1 = true, H2 = true, H3 = true }
    if methods then
        for _, name in ipairs({
            "SetTextColor", "SetSpacing", "SetFont", "SetFontObject", "SetShadowColor",
            "SetShadowOffset", "SetJustifyH", "SetJustifyV", "SetIndentedWordWrap",
            "GetTextColor", "GetSpacing", "GetFont", "GetFontObject", "GetShadowColor",
            "GetShadowOffset", "GetJustifyH", "GetJustifyV", "GetIndentedWordWrap",
        }) do
            local f = methods[name]
            if f then
                methods[name] = function(self, first, ...)
                    if type(first) == "string" and elements[first] then
                        return f(self, ...)
                    end
                    return f(self, first, ...)
                end
            end
        end
    end
end

-- A character without a capture has no Battle.net friends, no guild, no
-- group finder listings or applications and no pending spell prompts.
-- The generated stubs return nothing, where Blizzard code expects counts
-- and lists.
function BNGetNumFriends() return 0, 0, 0, 0 end
function GuildControlGetNumRanks() return 0 end
function GetSpellConfirmationPromptsInfo() return {} end
C_LFGList = C_LFGList or {}
function C_LFGList.GetNumApplications() return 0, 0 end
function C_LFGList.GetApplications() return {} end
function C_LFGList.GetAvailableCategories() return {} end
C_UnitAurasPrivate = C_UnitAurasPrivate or {}
function C_UnitAurasPrivate.GetPrivateAuraAnchors() return {} end
function C_UnitAurasPrivate.GetAllPrivateAuraInstanceIDs() return {} end

-- The client's text locale is the only one available. The locale ID is
-- inferred (0 for enUS).
function GetAvailableLocaleInfo()
    return { { localeId = 0, localeName = GetCVar("textLocale") or "enUS" } }
end

-- GetFontInfo(font): a font object's (or font object name's) settings.
function GetFontInfo(font)
    if type(font) == "string" then font = _G[font] end
    if type(font) ~= "table" or not font.GetFont then return nil end
    local _, height, outline = font:GetFont()
    local r, g, b, a = font:GetTextColor()
    local info = {
        fontObject = font,
        height = height or 0,
        outline = outline or "",
        color = CreateColor and CreateColor(r or 1, g or 1, b or 1, a or 1)
            or { r = r, g = g, b = b, a = a },
        canBeUserScaled = false,
    }
    if font.GetShadowOffset then
        local x, y = font:GetShadowOffset()
        local sr, sg, sb, sa = font:GetShadowColor()
        if (x and x ~= 0) or (y and y ~= 0) then
            info.shadow = { x = x, y = y,
                color = CreateColor and CreateColor(sr, sg, sb, sa) or { r = sr, g = sg, b = sb, a = sa } }
        end
    end
    return info
end

-- Not on a class trial, not in a scenario, no loot to roll for and no pet
-- battle. The service display data has no texture kit (the real one is
-- not known; Blizzard's texture kit helper skips a missing kit).
C_CharacterServices = C_CharacterServices or {}
function C_CharacterServices.GetCharacterServiceDisplayData()
    return { flowTitle = "", popupInfo = {} }
end
C_ClassTrial = C_ClassTrial or {}
function C_ClassTrial.GetClassTrialLogoutTimeSeconds() return 0 end
C_Scenario = C_Scenario or {}
function C_Scenario.GetInfo() return "", 0, 0, 0, false, false, false, 0, 0, 0 end
function GetActiveLootRollIDs() return {} end
C_PetBattles = C_PetBattles or {}
function C_PetBattles.GetHealth() return 0 end
function C_PetBattles.GetMaxHealth() return 0 end
-- No bonus objectives or world quest tasks are active.
function GetTasksTable() return {} end

-- The player has no alternate power bar timers. (The generated stub
-- returns bar ID 0, which Blizzard's loop takes for a timer.)
function UnitPowerBarTimerInfo() return nil end

-- Global strings that Blizzard code needs at load but that the game data
-- does not give the sandbox (probably in an encrypted section of
-- GlobalStrings.db2). The string's own name stands in; the game data
-- replaces it when present.
for _, name in ipairs({ "SOCIAL_ENABLE_DISCORD_FUNCTIONALITY" }) do
    if rawget(_G, name) == nil then rawset(_G, name, name) end
end


-- The client's default UI scale: 768 UI units over the physical screen
-- height (inferred from the classic "pixel perfect" scale; the client may
-- clamp it). The generated stub returns 0, which SetScale refuses.
function GetDefaultScale()
    local _, height = GetPhysicalScreenSize()
    if not height or height <= 0 then return 1 end
    return 768 / height
end

-- No addon performance warnings. The documentation says the result is
-- never nil, but Blizzard's code expects nil when there is no message
-- (the stub's empty message has an invalid type).
C_AddOnProfiler = C_AddOnProfiler or {}
function C_AddOnProfiler.CheckForPerformanceMessage() return nil end

-- Classes and races (ChrClasses and ChrRaces in the game data). English
-- names; a capture's localized names replace them in scenarios.
C_CreatureInfo = C_CreatureInfo or {}
local classes = {
    { "Warrior", "WARRIOR" }, { "Paladin", "PALADIN" }, { "Hunter", "HUNTER" },
    { "Rogue", "ROGUE" }, { "Priest", "PRIEST" }, { "Death Knight", "DEATHKNIGHT" },
    { "Shaman", "SHAMAN" }, { "Mage", "MAGE" }, { "Warlock", "WARLOCK" },
    { "Monk", "MONK" }, { "Druid", "DRUID" }, { "Demon Hunter", "DEMONHUNTER" },
    { "Evoker", "EVOKER" },
}
function C_CreatureInfo.GetClassInfo(classID)
    local c = classes[classID]
    if not c then return nil end
    return { className = c[1], classFile = c[2], classID = classID }
end
local races = {
    [1] = { "Human", "Human" }, [2] = { "Orc", "Orc" }, [3] = { "Dwarf", "Dwarf" },
    [4] = { "Night Elf", "NightElf" }, [5] = { "Undead", "Scourge" },
    [6] = { "Tauren", "Tauren" }, [7] = { "Gnome", "Gnome" }, [8] = { "Troll", "Troll" },
    [9] = { "Goblin", "Goblin" }, [10] = { "Blood Elf", "BloodElf" },
    [11] = { "Draenei", "Draenei" }, [22] = { "Worgen", "Worgen" },
    [24] = { "Pandaren", "Pandaren" }, [52] = { "Dracthyr", "Dracthyr" },
    [84] = { "Earthen", "EarthenDwarf" },
}
function C_CreatureInfo.GetRaceInfo(raceID)
    local r = races[raceID]
    if not r then return nil end
    return { raceName = r[1], clientFileString = r[2], raceID = raceID }
end

-- Click bindings (Blizzard's click casting). The profile starts with the
-- client's two default interactions, as in every character's
-- click-bindings-cache.txt: left click targets the unit, right click
-- opens its menu. SecureUnitButton_OnClick gives up on a target or menu
-- click unless GetBindingType reports the interaction. No modifier keys
-- are held in the sandbox, so MakeModifiers is 0.
C_ClickBindings = C_ClickBindings or {}
do
    local INTERACTION, TARGET, MENU = 3, 1, 2 -- Enum.ClickBindingType, ClickBindingInteraction
    local defaults = {
        { type = INTERACTION, actionID = TARGET, button = "LeftButton", modifiers = 0 },
        { type = INTERACTION, actionID = MENU, button = "RightButton", modifiers = 0 },
    }
    local profile
    local function copy(list)
        local out = {}
        for i, b in ipairs(list) do
            out[i] = { type = b.type, actionID = b.actionID, button = b.button, modifiers = b.modifiers }
        end
        return out
    end
    local function find(button, modifiers)
        for _, b in ipairs(profile or defaults) do
            if b.button == button and b.modifiers == (modifiers or 0) then return b end
        end
    end
    function MakeModifiers() return 0 end
    function C_ClickBindings.GetProfileInfo() return copy(profile or defaults) end
    function C_ClickBindings.SetProfileByInfo(info) profile = copy(info or {}) end
    function C_ClickBindings.ResetCurrentProfile() profile = nil end
    function C_ClickBindings.GetBindingType(button, modifiers)
        local b = find(button, modifiers)
        return b and b.type or 0
    end
    -- The button whose secure "type" attribute an interaction uses.
    function C_ClickBindings.GetEffectiveInteractionButton(button, modifiers)
        local b = find(button, modifiers)
        if b and b.type == INTERACTION then
            return b.actionID == TARGET and "LeftButton" or "RightButton"
        end
        return button
    end
    function C_ClickBindings.CanSpellBeClickBound() return true end
    function C_ClickBindings.ExecuteBinding(unit, button, modifiers)
        local b = find(button, modifiers)
        if not b then return end
        if b.type == 1 or b.type == 4 then
            CastSpellByID(b.actionID, unit)
        elseif b.type == 2 and RunMacro then
            RunMacro(b.actionID)
        end
    end
end

-- Equipment slots: GetInventorySlotInfo("HEADSLOT") returns the slot ID
-- and the empty slot's texture (the client's fixed table).
do
    local slots = {
        HEADSLOT = { 1, "Head" }, NECKSLOT = { 2, "Neck" }, SHOULDERSLOT = { 3, "Shoulder" },
        SHIRTSLOT = { 4, "Shirt" }, CHESTSLOT = { 5, "Chest" }, WAISTSLOT = { 6, "Waist" },
        LEGSSLOT = { 7, "Legs" }, FEETSLOT = { 8, "Feet" }, WRISTSLOT = { 9, "Wrists" },
        HANDSSLOT = { 10, "Hands" }, FINGER0SLOT = { 11, "Finger" }, FINGER1SLOT = { 12, "Finger" },
        TRINKET0SLOT = { 13, "Trinket" }, TRINKET1SLOT = { 14, "Trinket" }, BACKSLOT = { 15, "Chest" },
        MAINHANDSLOT = { 16, "MainHand" }, SECONDARYHANDSLOT = { 17, "SecondaryHand" },
        RANGEDSLOT = { 18, "Ranged" }, TABARDSLOT = { 19, "Tabard" },
        PROF0TOOLSLOT = { 20, "MainHand" }, PROF0GEAR0SLOT = { 21, "Chest" },
        PROF0GEAR1SLOT = { 22, "Chest" }, PROF1TOOLSLOT = { 23, "MainHand" },
        PROF1GEAR0SLOT = { 24, "Chest" }, PROF1GEAR1SLOT = { 25, "Chest" },
        COOKINGTOOLSLOT = { 26, "MainHand" }, COOKINGGEAR0SLOT = { 27, "Chest" },
        FISHINGTOOLSLOT = { 28, "MainHand" }, FISHINGGEAR0SLOT = { 29, "Chest" },
        FISHINGGEAR1SLOT = { 30, "Chest" },
        BAG0SLOT = { 31, "Bag" }, BAG1SLOT = { 32, "Bag" }, BAG2SLOT = { 33, "Bag" },
        BAG3SLOT = { 34, "Bag" }, REAGENTBAG0SLOT = { 35, "Bag" },
    }
    function GetInventorySlotInfo(name)
        local s = type(name) == "string" and slots[name:upper()]
        if not s then error("Invalid inventory slot in GetInventorySlotInfo", 2) end
        return s[1], "Interface\\PaperDoll\\UI-PaperDoll-Slot-" .. s[2], false
    end
end

-- Chat windows, as the client keeps them in chat-cache.txt. The defaults
-- are a new character's: General (shown) and Combat Log (docked second),
-- black at alpha 0.25, General taking the usual message groups. Not saved
-- between sessions yet.
do
    local GENERAL_MESSAGES = {
        "SYSTEM", "SYSTEM_NOMENU", "SAY", "EMOTE", "YELL", "WHISPER", "PARTY", "PARTY_LEADER",
        "RAID", "RAID_LEADER", "RAID_WARNING", "GUILD", "OFFICER", "MONSTER_SAY", "MONSTER_YELL",
        "MONSTER_EMOTE", "MONSTER_WHISPER", "MONSTER_BOSS_EMOTE", "MONSTER_BOSS_WHISPER",
        "ERRORS", "AFK", "DND", "IGNORED", "BG_HORDE", "BG_ALLIANCE", "BG_NEUTRAL",
        "COMBAT_FACTION_CHANGE", "SKILL", "LOOT", "MONEY", "CHANNEL", "ACHIEVEMENT",
        "GUILD_ACHIEVEMENT", "BN_WHISPER", "BN_WHISPER_INFORM", "BN_INLINE_TOAST_ALERT",
        "CURRENCY", "BN_WHISPER_PLAYER_OFFLINE", "PET_BATTLE_INFO", "INSTANCE_CHAT",
        "INSTANCE_CHAT_LEADER", "PING",
    }
    local windows = {}
    for i = 1, 10 do
        windows[i] = { name = "", size = 0, r = 0, g = 0, b = 0, a = 0.25, shown = false,
                       locked = true, docked = nil, uninteractable = false, messages = {}, channels = {} }
    end
    windows[1].name, windows[1].size, windows[1].shown, windows[1].docked = "General", 14, true, 1
    for _, m in ipairs(GENERAL_MESSAGES) do windows[1].messages[#windows[1].messages + 1] = m end
    windows[2].name, windows[2].size, windows[2].docked = "Combat Log", 14, 2

    local function w(i) return windows[tonumber(i) or 0] end
    function GetChatWindowInfo(i)
        local c = w(i)
        if not c then return nil end
        return c.name, c.size, c.r, c.g, c.b, c.a, c.shown, c.locked, c.docked, c.uninteractable
    end
    function GetChatWindowMessages(i)
        local c = w(i)
        if c then return unpack(c.messages) end
    end
    -- Channel name and zone channel flag, for each channel.
    function GetChatWindowChannels(i)
        local c = w(i)
        if not c then return end
        local out = {}
        for _, name in ipairs(c.channels) do out[#out + 1] = name; out[#out + 1] = 0 end
        return unpack(out)
    end
    function GetChatWindowSavedPosition(i)
        local c = w(i)
        if c and c.point then return c.point, c.x, c.y end
    end
    function GetChatWindowSavedDimensions(i)
        local c = w(i)
        if c and c.width then return c.width, c.height end
    end
    local function setter(field)
        return function(i, v) local c = w(i) if c then c[field] = v end end
    end
    SetChatWindowName, SetChatWindowSize = setter("name"), setter("size")
    SetChatWindowAlpha, SetChatWindowShown = setter("a"), setter("shown")
    SetChatWindowLocked, SetChatWindowDocked = setter("locked"), setter("docked")
    SetChatWindowUninteractable = setter("uninteractable")
    function SetChatWindowColor(i, r, g, b)
        local c = w(i)
        if c then c.r, c.g, c.b = r, g, b end
    end
    function SetChatWindowSavedPosition(i, point, x, y)
        local c = w(i)
        if c then c.point, c.x, c.y = point, x, y end
    end
    function SetChatWindowSavedDimensions(i, width, height)
        local c = w(i)
        if c then c.width, c.height = width, height end
    end
    local function addTo(list, value)
        for _, v in ipairs(list) do if v == value then return end end
        list[#list + 1] = value
    end
    local function removeFrom(list, value)
        for k = #list, 1, -1 do if list[k] == value then table.remove(list, k) end end
    end
    function AddChatWindowMessages(i, group) local c = w(i) if c then addTo(c.messages, group) end end
    function RemoveChatWindowMessages(i, group) local c = w(i) if c then removeFrom(c.messages, group) end end
    function AddChatWindowChannel(i, name) local c = w(i) if c then addTo(c.channels, name) end end
    function RemoveChatWindowChannel(i, name) local c = w(i) if c then removeFrom(c.channels, name) end end
end

-- No outdoor PvP zone has a battle timer. The documentation says the wait
-- time is never nil, but the world map shows "Next Battle" whenever it is
-- not nil, so the client must return nil outside such zones.
C_PvP = C_PvP or {}
function C_PvP.GetOutdoorPvPWaitTime() return nil end

-- The calendar: real months from the system clock, no events. The client
-- keeps a viewed month; GetMonthInfo(offset) counts from it (SetAbsMonth
-- and SetMonth move it). Weekdays count from 1 = Sunday.
C_Calendar = C_Calendar or {}
C_DateAndTime = C_DateAndTime or {}
do
    local viewedMonth, viewedYear
    local function today() return date("*t") end
    local function daysIn(month, year)
        local t = time({ year = year + math.floor(month / 12), month = month % 12 + 1, day = 0, hour = 12 })
        return tonumber(date("%d", t))
    end
    local function firstWeekday(month, year)
        return tonumber(date("%w", time({ year = year, month = month, day = 1, hour = 12 }))) + 1
    end
    local function shift(month, year, offset)
        local m = month - 1 + (offset or 0)
        return m % 12 + 1, year + math.floor(m / 12)
    end
    local function viewed()
        if not viewedMonth then
            local t = today()
            viewedMonth, viewedYear = t.month, t.year
        end
        return viewedMonth, viewedYear
    end
    local function calendarTime(t)
        return { year = t.year, month = t.month, monthDay = t.day, weekday = t.wday,
                 hour = t.hour, minute = t.min }
    end
    function C_DateAndTime.GetCurrentCalendarTime() return calendarTime(today()) end
    function C_Calendar.GetMonthInfo(offset)
        local m, y = viewed()
        m, y = shift(m, y, offset)
        return { month = m, year = y, numDays = daysIn(m, y), firstWeekday = firstWeekday(m, y) }
    end
    function C_Calendar.SetAbsMonth(month, year) viewedMonth, viewedYear = month, year end
    function C_Calendar.SetMonth(offset)
        local m, y = viewed()
        viewedMonth, viewedYear = shift(m, y, offset)
    end
    function C_Calendar.GetMinDate()
        return { year = 2004, month = 11, monthDay = 23, weekday = 3, hour = 0, minute = 0 }
    end
    function C_Calendar.GetMaxDate()
        local t = today()
        return { year = t.year + 1, month = 12, monthDay = 31, weekday = 1, hour = 23, minute = 59 }
    end
    C_Calendar.GetMaxCreateDate = C_Calendar.GetMaxDate
    function C_Calendar.GetNumDayEvents() return 0 end
    function C_Calendar.GetDayEvent() return nil end
    function C_Calendar.GetNumPendingInvites() return 0 end
    function C_Calendar.GetFirstPendingInvite() return nil end
    function C_Calendar.GetHolidayInfo() return nil end
    function C_Calendar.GetNumInvites() return 0 end
    function C_Calendar.GetEventIndex() return nil end
    function C_Calendar.OpenCalendar() end
    function C_Calendar.AreNamesReady() return true end
    function C_Calendar.IsActionPending() return false end
end

-- Settings: the client opens the panel by firing SETTINGS_PANEL_OPEN at
-- once (the event is synchronous in the API documentation).
C_SettingsUtil = C_SettingsUtil or {}
function C_SettingsUtil.OpenSettingsPanel(categoryID, scrollToElementName)
    fireNow("SETTINGS_PANEL_OPEN", categoryID, scrollToElementName)
end

-- The group finder is open to the player.
C_LFGInfo = C_LFGInfo or {}
function C_LFGInfo.CanPlayerUseGroupFinder() return true, "" end

-- Bags: an empty 16-slot backpack, no other bags (a new character's).
C_Container = C_Container or {}
do
    local BACKPACK_SLOTS = 16
    function C_Container.GetContainerNumSlots(bag)
        if bag == 0 then return BACKPACK_SLOTS end
        return 0
    end
    function C_Container.GetContainerNumFreeSlots(bag)
        if bag == 0 then return BACKPACK_SLOTS, 0 end
        return 0, 0
    end
    function C_Container.GetContainerItemInfo() return nil end
    function C_Container.GetContainerItemID() return nil end
    function C_Container.GetContainerItemLink() return nil end
end

-- Display: one monitor, the window at the physical screen size.
C_VideoOptions = C_VideoOptions or {}
do
    local function size()
        local w, h = GetPhysicalScreenSize()
        return CreateVector2D(w or 1920, h or 1080)
    end
    function GetMonitorCount() return 1 end
    function GetMonitorName() return "Monitor 1", true end
    function C_VideoOptions.GetCurrentGameWindowSize() return size() end
    function C_VideoOptions.GetDefaultGameWindowSize() return size() end
    function C_VideoOptions.GetGameWindowSizes() return { size() } end
    function C_VideoOptions.GetGxAdapterInfo()
        return { { name = "Sandbox renderer", isLowPower = false, isExternal = false } }
    end
end
