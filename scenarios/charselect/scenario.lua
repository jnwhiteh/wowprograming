-- Character select scenario (run with --glue).
--
-- The character list is read at run time from the real game account
-- folder (read only), so no personal data is stored here:
--
--   Syndicator.lua           name, realm, race, class, sex, faction, guild
--                            (from the Syndicator addon, if installed)
--   !WowProgrammingTrace.lua level, zone, GUID, spec and equipment of
--                            characters that logged in with the trace addon
--   character-list-order.txt the order of the character list, by GUID
--
-- Client constants (named colors, enums, CVars) come from the Evoker
-- capture; the glue screens use the same tables as the game.
local dir, S = ...

local capture = S.LoadCapture("../evoker/capture.txt")
if capture then
    if capture.constants then S.ApplyClientConstants(capture.constants) end
    for name, values in pairs(capture.enums or {}) do
        local e = {}
        for k, v in pairs(values) do e[k] = v end
        Enum[name] = e
    end
    S.ApplyCVars(capture.cvars)
    S.AddAtlases(capture.atlases)
end
local meta = capture and capture.meta or {}

-- Race file names to race IDs (ChrRaces).
local raceIDs = {
    Human = 1, Orc = 2, Dwarf = 3, NightElf = 4, Scourge = 5, Tauren = 6,
    Gnome = 7, Troll = 8, Goblin = 9, BloodElf = 10, Draenei = 11,
    Worgen = 22, Pandaren = 24, Nightborne = 27, HighmountainTauren = 28,
    VoidElf = 29, LightforgedDraenei = 30, ZandalariTroll = 31,
    KulTiran = 32, DarkIronDwarf = 34, Vulpera = 35, MagharOrc = 36,
    Mechagnome = 37, Dracthyr = 52, EarthenDwarf = 84, Harronir = 91,
}
local classIDs = {
    WARRIOR = 1, PALADIN = 2, HUNTER = 3, ROGUE = 4, PRIEST = 5,
    DEATHKNIGHT = 6, SHAMAN = 7, MAGE = 8, WARLOCK = 9, MONK = 10,
    DRUID = 11, DEMONHUNTER = 12, EVOKER = 13,
}

local function className(file, sex)
    local names = (sex == 3 and meta.localizedClassesFemale) or meta.localizedClasses
    return names and names[file] or file
end

-- Trace profiles: the newest per character.
local profiles = {}
local trace = S.ReadGameSavedVariables("!WowProgrammingTrace.lua")
for key, sessions in pairs(trace and trace.WowProgrammingTraceDB or {}) do
    for _, session in ipairs(sessions) do
        local p = session.profile
        if p and p.name and p.name ~= "" then
            local old = profiles[key]
            if not old or (p.time or 0) > (old.time or 0) then profiles[key] = p end
        end
    end
end

local characters = {}
local byKey = {}
local function add(key, info)
    if byKey[key] then
        for k, v in pairs(info) do
            if byKey[key][k] == nil then byKey[key][k] = v end
        end
        return
    end
    byKey[key] = info
    characters[#characters + 1] = info
end

-- Traced characters first: they have the most data.
for key, p in pairs(profiles) do
    add(key, {
        name = p.name, realmName = p.realm, guid = p.guid,
        experienceLevel = p.level, areaName = p.zone,
        raceFilename = p.race, raceName = p.raceName,
        classFilename = p.class, genderEnum = (p.sex or 2) - 2,
        faction = p.faction, specID = p.specID, items = p.items,
        mapID = p.mapID,
    })
end

local syndicator = S.ReadGameSavedVariables("Syndicator.lua")
local data = syndicator and syndicator.SYNDICATOR_DATA
for key, c in pairs(data and data.Characters or {}) do
    local d = c.details
    if d and d.character then
        add(d.character .. "-" .. (d.realmNormalized or d.realm or ""), {
            name = d.character, realmName = d.realm,
            raceFilename = d.race, raceName = d.race,
            classFilename = d.className, genderEnum = (d.sex or 2) - 2,
            faction = d.faction,
        })
    end
end

-- Fill in what neither source has.
local order = {}
local orderText = S.ReadGameFile("character-list-order.txt") or ""
for guid, index in orderText:gmatch("%d+ (%S+) (%d+)") do
    order["Player-" .. guid] = tonumber(index)
end
for i, c in ipairs(characters) do
    c.guid = c.guid or string.format("Player-0000-%08X", i)
    c.classID = classIDs[c.classFilename]
    c.className = className(c.classFilename, (c.genderEnum or 0) + 2)
    c.raceID = raceIDs[c.raceFilename]
    -- Not known without a trace of this character.
    c.experienceLevel = c.experienceLevel or 90
    c.areaName = c.areaName or ""
    c.genderID = c.genderEnum
    c.isGhost = false
    c.lastLoginBuild = 0
end
table.sort(characters, function(a, b)
    local oa, ob = order[a.guid] or 1000, order[b.guid] or 1000
    if oa ~= ob then return oa < ob end
    return a.name < b.name
end)
GlueCharacters = characters
print(string.format("Character select: %d characters", #characters))
