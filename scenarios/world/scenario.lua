-- World scenario: any character entered from character select that has no
-- scenario of its own. The player is described from the character list
-- (S.character: name, realm, class, race, sex, level, faction, zone).
-- Client constants come from the Evoker capture.
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

local c = S.character or {}
local classIDs = {
    WARRIOR = 1, PALADIN = 2, HUNTER = 3, ROGUE = 4, PRIEST = 5,
    DEATHKNIGHT = 6, SHAMAN = 7, MAGE = 8, WARLOCK = 9, MONK = 10,
    DRUID = 11, DEMONHUNTER = 12, EVOKER = 13,
}
-- Power by class: type, token, maximum.
local power = {
    WARRIOR = { 1, "RAGE", 100 }, ROGUE = { 3, "ENERGY", 100 },
    MONK = { 3, "ENERGY", 100 }, DRUID = { 0, "MANA", 250000 },
    DEATHKNIGHT = { 6, "RUNIC_POWER", 100 }, HUNTER = { 2, "FOCUS", 100 },
    DEMONHUNTER = { 17, "FURY", 100 }, EVOKER = { 0, "MANA", 250000 },
}
local classFile = c.class ~= "" and c.class or "WARRIOR"
local p = power[classFile] or { 0, "MANA", 250000 }
local names = meta.localizedClasses or {}

S.SetUnit("player", {
    name = c.name or "Player", level = c.level or 90,
    class = names[classFile] or classFile, classFile = classFile,
    classID = classIDs[classFile],
    race = c.race or "Human", raceFile = c.race or "Human", sex = (c.sex or 0) + 2,
    health = 750000, healthMax = 750000,
    power = p[3], powerMax = p[3], powerType = p[1], powerToken = p[2],
    isPlayer = true, reaction = 5, faction = c.faction, role = "DAMAGER",
    guid = c.guid,
})
print(string.format("World scenario: %s, level %s %s", tostring(c.name),
    tostring(c.level), classFile))
