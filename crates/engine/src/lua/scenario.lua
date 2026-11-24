-- Helpers for scenario files. Called with the scenario folder reader and
-- the decoders; returns the table that scenario.lua receives as its second
-- argument.
local readFile, decodeBase64, decompress, decodeJSON, addAtlas, returnMixins, fireEvent, readGameFile = ...
local type, pairs, ipairs, select, tostring, tonumber, unpack, rawget, _G =
      type, pairs, ipairs, select, tostring, tonumber, unpack, rawget, _G

local S = {}

-- Read a capture file: JSON, or the compressed Base64 text that the
-- in-game capture scripts produce.
function S.LoadCapture(name)
    local text = readFile(name)
    if not text then return nil end
    text = text:gsub("^%s+", ""):gsub("%s+$", "")
    if text:sub(1, 1) ~= "{" then
        local raw = decodeBase64(text)
        text = raw and decompress(raw)
        if not text then error("cannot decode capture " .. name) end
    end
    return decodeJSON(text)
end

-- Read a SavedVariables file of the real game account (read only), such
-- as "Syndicator.lua". Returns a table of the globals it defines, or nil.
function S.ReadGameSavedVariables(file)
    local code = readGameFile and readGameFile("SavedVariables/" .. file)
    if not code then return nil end
    local chunk, err = loadstring(code, "@" .. file)
    if not chunk then
        print("cannot read " .. file .. ": " .. tostring(err))
        return nil
    end
    local env = {}
    setfenv(chunk, env)
    local ok, runErr = pcall(chunk)
    if not ok then print("cannot read " .. file .. ": " .. tostring(runErr)) end
    return env
end

-- Read a text file of the real game account (read only).
function S.ReadGameFile(name)
    return readGameFile and readGameFile(name)
end

local function argKey(...)
    local parts = {}
    for i = 1, select("#", ...) do parts[i] = tostring((select(i, ...))) end
    return table.concat(parts, ",")
end

-- A fresh copy of a recorded table: Blizzard code often changes the tables
-- the client returns, which must not change the record.
local function copyValue(v)
    if type(v) ~= "table" then return v end
    local t = {}
    for k, x in pairs(v) do t[k] = copyValue(x) end
    return t
end

local function unpackRecord(rec)
    local n = tonumber(rec.n) or 0
    local out = {}
    for i = 1, n do
        local v = rec[tostring(i)]
        if v == nil then v = rec[i] end
        out[i] = copyValue(v)
    end
    return unpack(out, 1, n)
end

-- Path "C_Spell.GetSpellInfo" -> container table and key.
local function locate(path)
    local container, key = _G, path
    for part, rest in path:gmatch("([^.]+)%.(.+)") do
        local t = rawget(container, part)
        if type(t) ~= "table" then
            t = {}
            container[part] = t
        end
        container, key = t, rest
    end
    return container, key
end

-- Give recorded tables back the methods of their documented mixins.
local function withMixins(mixins, ...)
    local out = { ... }
    local n = select("#", ...)
    for i, name in pairs(mixins) do
        local mixin = rawget(_G, name)
        if type(out[i]) == "table" and type(mixin) == "table" then
            local copy = {}
            for k, v in pairs(out[i]) do copy[k] = v end
            for k, v in pairs(mixin) do
                if copy[k] == nil then copy[k] = v end
            end
            out[i] = copy
        end
    end
    return unpack(out, 1, n)
end

S.replayed = {}

-- Recorded functions that must keep the sandbox's own version: addon
-- loading (a replayed LoadAddOn never loads the addon) and functions that
-- act instead of report (Set..., Register..., WrapTextInColor, ...).
local NO_REPLAY_NAMESPACES = { C_AddOns = true }
local NO_REPLAY_VERBS = { "Set", "Add", "Remove", "Register", "Unregister", "Enable",
    "Disable", "Load", "Wrap", "Request" }

-- Functions that return objects the sandbox owns (widget metatables, frames).
-- A capture stores them as plain tables, which would replace the real ones.
-- Edit Mode layouts live in the sandbox's own store (seeded from the
-- capture by S.ApplyCapture), so saving a layout works.
local NO_REPLAY_NAMES = { GetCurrentKeyBoardFocus = true, ["C_EditMode.GetLayouts"] = true }

local function replayable(path)
    local namespace, name = path:match("^([^.]+)%.(.+)$")
    if namespace and NO_REPLAY_NAMESPACES[namespace] then return false end
    if NO_REPLAY_NAMES[path] or path:match("^Get%a+Metatable$") then return false end
    name = name or path
    for _, verb in ipairs(NO_REPLAY_VERBS) do
        if name:sub(1, #verb) == verb then return false end
    end
    return true
end

-- True if a recorded value holds "<secret>": the game hid it from the
-- capture, so the record is useless.
local function hasSecret(v)
    if v == "<secret>" then return true end
    if type(v) == "table" then
        for _, x in pairs(v) do
            if hasSecret(x) then return true end
        end
    end
    return false
end

-- Replace functions with recorded answers. Calls with arguments that were
-- not recorded fall back to the previous function (usually a stub), and so
-- do calls whose recorded answer was secret.
function S.Replay(calls)
    local count = 0
    for path, all in pairs(calls) do
        local records = {}
        for args, rec in pairs(all) do
            if args ~= "#" and not hasSecret(rec) then records[args] = rec end
        end
        if replayable(path) and next(records) ~= nil then
            local container, key = locate(path)
            local fallback = container[key]
            container[key] = function(...)
                local rec = records[argKey(...)]
                if rec == nil then
                    -- Calls often pass optional arguments the capture did not
                    -- use: try shorter argument lists.
                    local args = { ... }
                    local n = select("#", ...) - 1
                    while rec == nil and n >= 0 do
                        rec = records[argKey(unpack(args, 1, n))]
                        n = n - 1
                    end
                end
                if rec ~= nil then
                    local mixins = returnMixins and returnMixins[path]
                    if mixins then return withMixins(mixins, unpackRecord(rec)) end
                    return unpackRecord(rec)
                end
                if fallback then return fallback(...) end
            end
            S.replayed[path] = true
            count = count + 1
        end
    end
    return count
end

-- Define a function or value at a path such as "C_Spell.IsSpellDataCached".
function S.Set(path, value)
    local container, key = locate(path)
    container[key] = value
end

-- Merge client constants: colors, Enum, Constants and number globals.
function S.ApplyClientConstants(data)
    if data.colors then
        local colors = {}
        for i, c in ipairs(data.colors) do
            colors[i] = { baseTag = c.baseTag, color = { r = c.r, g = c.g, b = c.b, a = c.a } }
        end
        C_UIColor.GetColors = function() return colors end
    end
    local function merge(target, source)
        for k, v in pairs(source) do
            local key = tonumber(k) or k
            if type(v) == "table" then
                if type(target[key]) ~= "table" then target[key] = {} end
                merge(target[key], v)
            elseif target[key] == nil then
                target[key] = v
            end
        end
    end
    if data.enum then merge(Enum, data.enum) end
    if data.constants then merge(Constants, data.constants) end
    if data.numbers then
        for k, v in pairs(data.numbers) do
            if rawget(_G, k) == nil then _G[k] = v end
        end
    end
end

-- Set CVars from a capture: name -> { value, default }.
function S.ApplyCVars(cvars)
    local n = 0
    for name, info in pairs(cvars or {}) do
        if type(info) == "table" then
            if info.default ~= nil then C_CVar.RegisterCVar(name, info.default) end
            if info.value ~= nil then C_CVar.SetCVar(name, info.value) end
            n = n + 1
        end
    end
    return n
end

-- Fill the sandbox's Edit Mode store from captured C_EditMode.GetLayouts
-- answers, unless the sandbox already saved layouts. The capture may hold
-- the table after Blizzard added its presets to it; presets are skipped.
function S.SeedEditModeLayouts(records)
    if type(records) ~= "table" then return end
    local info
    for args, rec in pairs(records) do
        if args ~= "#" and type(rec) == "table" then info = unpackRecord(rec) break end
    end
    if type(info) ~= "table" or type(info.layouts) ~= "table" then return end
    local account = rawget(_G, "WowProgramming_EditModeAccount")
    local character = rawget(_G, "WowProgramming_EditModeCharacter")
    local saved = (type(account) == "table" and #(account.layouts or {}) > 0)
        or (type(character) == "table" and #(character.layouts or {}) > 0)
    if saved then return end
    account = type(account) == "table" and account or { settings = {} }
    character = type(character) == "table" and character or {}
    account.layouts, character.layouts = {}, {}
    local accountType = Enum and Enum.EditModeLayoutType and Enum.EditModeLayoutType.Account or 1
    local characterType = Enum and Enum.EditModeLayoutType and Enum.EditModeLayoutType.Character or 2
    for _, l in ipairs(info.layouts) do
        if l.layoutType == accountType then
            account.layouts[#account.layouts + 1] = l
        elseif l.layoutType == characterType then
            character.layouts[#character.layouts + 1] = l
        end
    end
    -- The active index counts Blizzard's presets (Modern and Classic) first.
    local presets = 2
    local own = #account.layouts + #character.layouts
    local active = tonumber(info.activeLayout) or 1
    character.activeLayout = (active <= presets + own) and active or 1
    rawset(_G, "WowProgramming_EditModeAccount", account)
    rawset(_G, "WowProgramming_EditModeCharacter", character)
end

-- Apply everything a WowProgrammingCapture addon capture holds that does
-- not depend on the scenario: constants, enums, CVars, atlases and the
-- recorded API answers. Returns the number of replayed functions.
function S.ApplyCapture(capture)
    if capture.constants then S.ApplyClientConstants(capture.constants) end
    for name, values in pairs(capture.enums or {}) do
        local e = {}
        for k, v in pairs(values) do e[k] = v end
        Enum[name] = e
    end
    S.ApplyCVars(capture.cvars)
    S.AddAtlases(capture.atlases)
    S.SeedEditModeLayouts(capture.calls and capture.calls["C_EditMode.GetLayouts"])
    return capture.calls and S.Replay(capture.calls) or 0
end

-- Add atlases that the game tables lack, from a capture's atlases table
-- (name -> C_Texture.GetAtlasInfo result). Returns how many were added.
function S.AddAtlases(atlases)
    local n = 0
    for name, info in pairs(atlases or {}) do
        if type(info) == "table" and addAtlas(name, info) then n = n + 1 end
    end
    return n
end

-- Load capture.txt (and client_constants.txt for old captures), apply it,
-- and set up what every captured scenario needs. `label` names the scenario
-- in messages. Sets WowProgrammingCapture for tools such as
-- tools/compare/compare_frames.lua. Returns the capture or nil.
function S.LoadStandardCapture(label)
    local capture = S.LoadCapture("capture.txt")
    local meta = capture and capture.meta or {}
    local n = 0
    if capture and (meta.captureVersion or 0) >= 2 then
        n = S.ApplyCapture(capture)
    else
        -- Version 1: spellbook_capture.lua and client_constants_capture.lua.
        local constants = S.LoadCapture("client_constants.txt")
        if constants then S.ApplyClientConstants(constants) end
        if capture then
            n = S.Replay(capture.calls)
            S.AddAtlases(capture.atlases)
        end
    end
    if not capture then
        print(label .. " scenario: capture.txt is missing")
        return nil
    end
    print(string.format("%s scenario: replaying %d captured functions", label, n))
    WowProgrammingCapture = capture

    -- Open the spellbook in the same state as in the capture.
    if meta.spellBookMinimized ~= nil then
        SetCVar("spellBookMinimize", meta.spellBookMinimized and "1" or "0")
    end
    -- Class colors and names.
    if meta.classColors and not S.replayed["C_ClassColor.GetClassColor"] then
        local colors = meta.classColors
        S.Set("C_ClassColor.GetClassColor", function(class)
            local c = colors[class]
            if c and CreateColor then return CreateColor(c.r, c.g, c.b, c.a) end
            return c
        end)
    end
    if meta.localizedClasses then
        S.Set("LocalizedClassList", function(female)
            return female and meta.localizedClassesFemale or meta.localizedClasses
        end)
    end

    -- Spell data is always loaded in the sandbox.
    S.Set("C_Spell.IsSpellDataCached", function() return true end)
    S.Set("C_Spell.RequestLoadSpellData", function() end)
    -- Older captures lack these; derive or assume them.
    if not S.replayed["GetSpecializationRoleEnum"] then
        local roleEnum = { TANK = Enum.LFGRole.Tank, HEALER = Enum.LFGRole.Healer,
            DAMAGER = Enum.LFGRole.Damage }
        S.Set("GetSpecializationRoleEnum", function(index, ...)
            local role = select(5, GetSpecializationInfo(index, ...))
            return roleEnum[role]
        end)
    end
    if not S.replayed["C_PetJournal.GetNumPetsNeedingFanfare"] then
        S.Set("C_PetJournal.GetNumPetsNeedingFanfare", function() return 0 end)
    end
    return capture
end

-- Deliver an event to the frames registered for it, as the client does.
S.FireEvent = fireEvent

-- Units described by the scenario --------------------------------------------
--
-- S.SetUnit(token, data) makes a unit exist with the given answers. Fields
-- (all optional): name, realm, level, class (localized), classFile,
-- classID, race, raceFile, sex, health, healthMax, power, powerMax,
-- powerType, powerToken, reaction (1 hostile .. 8 exalted), isPlayer,
-- classification ("normal", "elite", "worldboss", ...), creatureType,
-- faction ("Alliance", "Horde"), role ("TANK", "HEALER", "DAMAGER",
-- "NONE"), guid, dead, ghost, connected (default true), canAttack.
-- Units given here answer before captured answers and defaults.

local units = {}
S.units = units

local function unitData(unit)
    if type(unit) ~= "string" then return nil end
    return units[unit:lower()]
end

-- Wrap a global so described units answer first.
local function unitFunction(name, answer)
    local container, key = locate(name)
    local previous = container[key]
    container[key] = function(unit, ...)
        local u = unitData(unit)
        if u then return answer(u, ...) end
        if previous then return previous(unit, ...) end
    end
end

local wrapped = false
local function wrapUnitFunctions()
    if wrapped then return end
    wrapped = true
    unitFunction("UnitExists", function() return true end)
    unitFunction("UnitIsVisible", function() return true end)
    unitFunction("UnitName", function(u) return u.name or "Unknown", u.realm end)
    unitFunction("UnitNameUnmodified", function(u) return u.name or "Unknown", u.realm end)
    unitFunction("GetUnitName", function(u, showServer)
        if showServer and u.realm then return u.name .. "-" .. u.realm end
        return u.name or "Unknown"
    end)
    unitFunction("UnitFullName", function(u) return u.name or "Unknown", u.realm end)
    unitFunction("UnitGUID", function(u) return u.guid end)
    unitFunction("UnitLevel", function(u) return u.level or 1 end)
    unitFunction("UnitEffectiveLevel", function(u) return u.level or 1 end)
    unitFunction("UnitClass", function(u) return u.class, u.classFile, u.classID end)
    unitFunction("UnitClassBase", function(u) return u.classFile, u.classID end)
    unitFunction("UnitRace", function(u) return u.race, u.raceFile, u.raceID end)
    unitFunction("UnitSex", function(u) return u.sex or 2 end)
    unitFunction("UnitHealth", function(u) return u.health or u.healthMax or 100 end)
    unitFunction("UnitHealthMax", function(u) return u.healthMax or 100 end)
    unitFunction("UnitPower", function(u) return u.power or u.powerMax or 100 end)
    unitFunction("UnitPowerMax", function(u) return u.powerMax or 100 end)
    unitFunction("UnitPowerType", function(u)
        return u.powerType or 0, u.powerToken or "MANA", 0, 0, 0
    end)
    unitFunction("UnitReaction", function(u) return u.reaction or 4 end)
    unitFunction("UnitIsPlayer", function(u) return u.isPlayer == true end)
    unitFunction("UnitPlayerControlled", function(u) return u.isPlayer == true end)
    unitFunction("UnitClassification", function(u) return u.classification or "normal" end)
    unitFunction("UnitCreatureType", function(u) return u.creatureType end)
    unitFunction("UnitFactionGroup", function(u) return u.faction, u.faction end)
    unitFunction("UnitGroupRolesAssigned", function(u) return u.role or "NONE" end)
    unitFunction("UnitGroupRolesAssignedEnum", function(u)
        local roles = { TANK = "Tank", HEALER = "Healer", DAMAGER = "Damage" }
        local key = roles[u.role or "NONE"]
        return key and Enum.LFGRole[key] or nil
    end)
    unitFunction("UnitIsConnected", function(u) return u.connected ~= false end)
    unitFunction("UnitIsDead", function(u) return u.dead == true end)
    unitFunction("UnitIsGhost", function(u) return u.ghost == true end)
    unitFunction("UnitIsDeadOrGhost", function(u) return u.dead == true or u.ghost == true end)
    unitFunction("UnitCanAttack", function(u) return u.canAttack == true end)
    unitFunction("UnitIsEnemy", function(u) return (u.reaction or 4) <= 2 end)
    unitFunction("UnitIsFriend", function(u) return (u.reaction or 4) >= 5 end)
    unitFunction("UnitIsTapDenied", function(u) return u.tapDenied == true end)
    unitFunction("UnitIsPVP", function(u) return u.pvp == true end)
    unitFunction("UnitInParty", function(u) return u.inParty == true end)
    unitFunction("UnitInRaid", function(u) return u.raidIndex end)
    unitFunction("UnitSelectionColor", function(u)
        local r = u.reaction or 4
        if r <= 2 then return 1, 0, 0, 1 elseif r == 4 then return 1, 1, 0, 1 end
        return 0, 1, 0, 1
    end)
    -- UnitIsUnit compares tokens and GUIDs of described units.
    local previousIsUnit = UnitIsUnit
    UnitIsUnit = function(a, b)
        local ua, ub = unitData(a), unitData(b)
        if ua and ub then return ua == ub end
        if previousIsUnit then return previousIsUnit(a, b) end
        return false
    end
end

function S.SetUnit(token, data)
    wrapUnitFunctions()
    data.token = token
    units[token:lower()] = data
    return data
end

-- Action bar slots described by the scenario ----------------------------------
--
-- S.SetAction(slot, data): data.type ("spell", "item", "macro"; default
-- "spell"), data.id, data.texture (file ID or path), data.count, data.text,
-- data.usable (default true), data.cooldown = { start, duration }.

local actions = {}
S.actions = actions

local function actionFunction(name, answer)
    local container, key = locate(name)
    local previous = container[key]
    container[key] = function(slot, ...)
        local a = actions[slot]
        if a then return answer(a, ...) end
        if previous then return previous(slot, ...) end
    end
end

local actionsWrapped = false
function S.SetAction(slot, data)
    if not actionsWrapped then
        actionsWrapped = true
        actionFunction("C_ActionBar.HasAction", function() return true end)
        actionFunction("HasAction", function() return true end)
        actionFunction("GetActionInfo", function(a) return a.type or "spell", a.id, a.subType end)
        actionFunction("C_ActionBar.GetActionTexture", function(a) return a.texture end)
        actionFunction("C_ActionBar.GetActionText", function(a) return a.text end)
        actionFunction("C_ActionBar.GetActionDisplayCount", function(a)
            return a.count and tostring(a.count) or ""
        end)
        actionFunction("C_ActionBar.IsUsableAction", function(a) return a.usable ~= false, false end)
        actionFunction("C_ActionBar.GetActionCooldown", function(a)
            local cd = a.cooldown or {}
            return { startTime = cd[1] or 0, duration = cd[2] or 0, isEnabled = true, modRate = 1 }
        end)
        actionFunction("C_ActionBar.GetSpell", function(a)
            if (a.type or "spell") == "spell" then return a.id end
        end)
    end
    actions[slot] = data
end

-- Stand-ins for scenarios that load only the spellbook's part of the UI.
function S.SpellbookStandIns()
    S.Set("StoreFrame_IsShown", function() return false end)
    -- The real bars come from Blizzard_ActionBar, which pulls in most of the
    -- game UI; empty frames are enough for "is this spell on a bar" checks.
    for _, name in ipairs({ "MainActionBar", "MultiBarBottomLeft", "MultiBarBottomRight",
            "MultiBarRight", "MultiBarLeft", "MultiBar5", "MultiBar6", "MultiBar7" }) do
        local bar = CreateFrame("Frame", name, UIParent)
        bar.SetShowGrid = function() end
        bar.IsInDefaultPosition = function() return true end
    end
    -- The settings panel comes from Blizzard_Settings_Shared; the micro menu
    -- only asks whether it is shown.
    CreateFrame("Frame", "SettingsPanel", UIParent):Hide()
    CurrentVersionHasNewUnseenSettings = function() return false end
    -- Hovering a spell updates the highlights of the action buttons.
    ActionBarButtonEventsFrame = CreateFrame("Frame", "ActionBarButtonEventsFrame")
    ActionBarButtonEventsFrame.frames = {}
    -- Pet spells highlight the pet bar; Escape closes the bags.
    PetActionBar = CreateFrame("Frame", "PetActionBar", UIParent)
    PetActionBar.Update = function() end
    PetActionBar.UpdatePetActionHighlightMarks = function() end
    PetActionBar.ClearPetActionHighlightMarks = function() end
    CloseAllBags = function() return false end
    -- The Specialization tab borrows the cast bar from Blizzard_UIPanels_Game.
    local castBar = CreateFrame("StatusBar", "OverlayPlayerCastingBarFrame", UIParent)
    castBar:Hide()
    castBar.StartReplacingPlayerBarAt = function() end
    castBar.EndReplacingPlayerBar = function() end
    -- Edit mode would place HUD frames this part of the UI does not have.
    local f = CreateFrame("Frame")
    f:RegisterEvent("PLAYER_LOGIN")
    f:SetScript("OnEvent", function(self)
        self:UnregisterAllEvents()
        if EditModeManagerFrame then
            EditModeManagerFrame:UnregisterEvent("EDIT_MODE_LAYOUTS_UPDATED")
        end
    end)
end

return S
