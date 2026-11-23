-- Client functions of the glue environment (login, realm list, character
-- select), written in Lua. The sandbox is always logged in and connected
-- to a realm, so GlueParent goes straight to character select.

local fireNow = ...
-- Events that answer a request arrive on a later frame in the client, not
-- inside the call (deep nesting overflows the C stack otherwise).
local function fireEvent(event, ...)
    local n, args = select("#", ...), { ... }
    C_Timer.After(0, function() fireNow(event, unpack(args, 1, n)) end)
end

LE_AURORA_STATE_NONE = LE_AURORA_STATE_NONE or 0
LE_WOW_CONNECTION_STATE_NONE = LE_WOW_CONNECTION_STATE_NONE or 0
LE_WOW_CONNECTION_STATE_CONNECTING = LE_WOW_CONNECTION_STATE_CONNECTING or 1
LE_WOW_CONNECTION_STATE_CONNECTED = LE_WOW_CONNECTION_STATE_CONNECTED or 2

C_Login = C_Login or {}
-- auroraState, connectedToWoW, wowConnectionState, hasRealmList,
-- waitingForRealmList
function C_Login.GetState()
    return LE_AURORA_STATE_NONE, true, LE_WOW_CONNECTION_STATE_CONNECTED, false, false
end
function C_Login.IsLoginReady() return true end
function C_Login.IsLauncherLogin() return true end
function C_Login.GetLastError() return nil end
function C_Login.ClearLastError() end

-- The character list. A scenario fills GlueCharacters with one table per
-- character, using the field names of GetBasicCharacterInfo (name,
-- raceName, raceFilename, className, classFilename, classID,
-- experienceLevel, areaName, genderEnum, guid, faction, specID, raceID,
-- realmName, ...). The field list comes from the usage strings in Wow.exe.
GlueCharacters = GlueCharacters or {}
local selectedIndex = 1

local function byGUID(guid)
    for i, c in ipairs(GlueCharacters) do
        if c.guid == guid then return c, i end
    end
end

function GetNumCharacters(includeEmptySlots)
    return #GlueCharacters
end
function GetCharacterGUID(index)
    local c = GlueCharacters[index]
    return c and c.guid
end
function GetBasicCharacterInfo(guid)
    local c = byGUID(guid)
    local info = {}
    if c then
        for k, v in pairs(c) do info[k] = v end
    end
    return info
end
function GetServiceCharacterInfo(guid)
    return {
        boostInProgress = false, isTrialBoost = false, isTrialBoostCompleted = false,
        isRevokedCharacterUpgrade = false, vasServiceInProgress = false,
        isExpansionTrialCharacter = false, isLockedByExpansion = false,
        hasCustomize = false, hasRaceChange = false, hasFactionChange = false,
        hasNameChange = false, isLocked = false, customizeDisabled = false,
        characterServiceRequiresLogin = false, rpeArathiAvailable = false,
        hasWowToken = false, hasVasRevoked = false, mailSenders = {},
    }
end

-- The 45 values of GetCharacterInfo, in the order of the usage string.
local infoOrder = {
    "name", "raceName", "raceFilename", "className", "classFilename", "classID",
    "experienceLevel", "areaName", "genderEnum", "isGhost", "hasCustomize",
    "hasRaceChange", "hasFactionChange", "deprecated1", "guid", "profession0",
    "profession1", "genderID", "boostInProgress", "hasNameChange", "isLocked",
    "isTrialBoost", "isTrialBoostCompleted", "isRevokedCharacterUpgrade",
    "vasServiceInProgress", "lastLoginBuild", "specID", "isExpansionTrialCharacter",
    "faction", "isLockedByExpansion", "mailSenders", "customizeDisabled",
    "deprecated2", "characterServiceRequiresLogin", "lastActiveDay",
    "lastActiveMonth", "lastActiveYear", "raceID", "isLockedFromOtherCharErrors",
    "rpeArathiAvailable", "noRPEReason", "hasWowToken", "hasVasRevoked",
    "realmName", "maxLevelBoostUsed",
}
local function infoValues(guid)
    if not guid then return nil end
    local info = GetBasicCharacterInfo(guid)
    local service = GetServiceCharacterInfo(guid)
    local out = {}
    for i, key in ipairs(infoOrder) do
        local v = info[key]
        if v == nil then v = service[key] end
        out[i] = v
    end
    return unpack(out, 1, #infoOrder)
end
function GetCharacterInfo(index)
    return infoValues(GetCharacterGUID(index))
end
function GetCharacterInfoByGUID(guid)
    return infoValues(guid)
end
function GetCharacterListGroupsInfo() return {} end
function GetCharacterListUpdate()
    fireEvent("CHARACTER_LIST_UPDATE", #GlueCharacters)
end
-- The client answers with UPDATE_SELECTED_CHARACTER, and remembers the
-- choice in the lastCharacterIndex CVar (0-based).
function SelectCharacter(index)
    selectedIndex = index
    if index and index > 0 then
        SetCVar("lastCharacterIndex", index - 1)
    end
    fireEvent("UPDATE_SELECTED_CHARACTER", index)
end
function GetCharacterSelection() return selectedIndex end
function GetSelectedCharacterGUID() return GetCharacterGUID(selectedIndex) end

-- Glue events are not in the API documentation; accept them as valid.
local glueEvents = {
    FRAMES_LOADED = true, LOGIN_STATE_CHANGED = true, CHARACTER_LIST_UPDATE = true,
    UPDATE_SELECTED_CHARACTER = true, OPEN_STATUS_DIALOG = true,
    UPDATE_STATUS_DIALOG = true, CLIENT_ACCOUNT_MISMATCH = true,
    TRIAL_STATUS_UPDATE = true, REALM_LIST_UPDATED = true,
    CHARACTER_SELECT_LOGIN_STATE_CHANGED = true,
}
if C_EventUtils and C_EventUtils.IsEventValid then
    local isValid = C_EventUtils.IsEventValid
    function C_EventUtils.IsEventValid(name)
        return glueEvents[name] or isValid(name)
    end
end

-- The default game mode has no special glue screen. (The generated stub
-- returns "", which GlueParent takes as a screen name.)
C_GameRules = C_GameRules or {}
function C_GameRules.GetGameModeGlueScreenName() return nil end

-- InGlue() in Blizzard_SharedXMLBase asks the client.
C_Glue = C_Glue or {}
function C_Glue.IsOnGlueScreen() return true end

-- Warband groups: up to this many (the generated stub returns nothing).
function GetMaxWarbandGroupCount() return 6 end

-- Undocumented glue functions that return strings or tables. (Generated
-- stubs return nothing.)
function GetSavedAccountName() return "" end
function GetSavedAccountList() return "" end
C_CharacterServices = C_CharacterServices or {}
function C_CharacterServices.GetCharacterServiceDisplayInfo() return {} end
C_CharacterCreation = C_CharacterCreation or {}
function C_CharacterCreation.GetAvailableRaces() return {} end
C_SharedCharacterServices = C_SharedCharacterServices or {}
function C_SharedCharacterServices.GetUpgradeDistributions() return {} end
function C_CharacterServices.GetVASDistributions() return {} end

-- Glue enums missing from the API documentation. Values are placeholders
-- (the names are in Wow.exe; the values are not known).
Enum.CharacterCreateType = Enum.CharacterCreateType or { Normal = 0, Boost = 1, TrialBoost = 2 }
function C_CharacterCreation.GetCharacterCreateType() return Enum.CharacterCreateType.Normal end
