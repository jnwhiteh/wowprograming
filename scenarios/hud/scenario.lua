-- HUD scenario: the default UI with a player, a target and a party,
-- described here instead of captured. Use it to work on the player and
-- target frames, party and raid frames and action bars. Units on the
-- player's realm have no realm name, as in the client.
local dir, S = ...

-- Named colors, enums and constants captured from the client (a copy of
-- the Monk scenario's). A capture.txt from the WowProgrammingCapture addon
-- replaces them, if present.
local capture = S.LoadCapture("capture.txt")
if capture then
    S.ApplyCapture(capture)
    WowProgrammingCapture = capture
else
    S.ApplyClientConstants(S.LoadCapture("client_constants.txt"))
end

S.SetUnit("player", {
    name = "Monk", level = 90,
    class = "Monk", classFile = "MONK", classID = 10,
    race = "Pandaren", raceFile = "Pandaren", sex = 3,
    health = 412000, healthMax = 412000,
    power = 100, powerMax = 100, powerType = 3, powerToken = "ENERGY",
    isPlayer = true, reaction = 5, faction = "Alliance", role = "DAMAGER",
    guid = "Player-0000-00000001",
})
S.SetUnit("target", {
    name = "Training Dummy", level = 90, classification = "normal",
    creatureType = "Mechanical",
    health = 2740000, healthMax = 3800000,
    power = 0, powerMax = 0, powerType = 0, powerToken = "MANA",
    reaction = 2, canAttack = true,
    guid = "Creature-0-0000-0000-00000-31144-0000000001",
})
local party = {
    { name = "Shieldwall", class = "Warrior", classFile = "WARRIOR", classID = 1,
      race = "Human", raceFile = "Human", role = "TANK", powerType = 1, powerToken = "RAGE",
      health = 520000, healthMax = 600000, power = 30, powerMax = 100 },
    { name = "Leafwhisper", class = "Druid", classFile = "DRUID", classID = 11,
      race = "Night Elf", raceFile = "NightElf", role = "HEALER", powerType = 0, powerToken = "MANA",
      health = 380000, healthMax = 380000, power = 180000, powerMax = 250000 },
    { name = "Emberfall", class = "Mage", classFile = "MAGE", classID = 8,
      race = "Gnome", raceFile = "Gnome", role = "DAMAGER", powerType = 0, powerToken = "MANA",
      health = 150000, healthMax = 340000, power = 90000, powerMax = 250000 },
    { name = "Nightshade", class = "Rogue", classFile = "ROGUE", classID = 4,
      race = "Dwarf", raceFile = "Dwarf", role = "DAMAGER", powerType = 3, powerToken = "ENERGY",
      health = 0, healthMax = 360000, power = 0, powerMax = 100, dead = true },
}
for i, member in ipairs(party) do
    member.level = 90
    member.isPlayer = true
    member.reaction = 5
    member.faction = "Alliance"
    member.inParty = true
    member.guid = "Player-0000-0000000" .. (i + 1)
    S.SetUnit("party" .. i, member)
end
S.units.player.inParty = true

S.Set("IsInGroup", function() return true end)
S.Set("IsInRaid", function() return false end)
S.Set("GetNumGroupMembers", function() return #party + 1 end)
S.Set("GetNumSubgroupMembers", function() return #party end)
S.Set("UnitIsGroupLeader", function(unit) return unit == "player" end)

-- Windwalker spells on the main action bar; icons as the Monk capture
-- recorded them. Rising Sun Kick shows a cooldown.
local spells = {
    { 100780, 606551 },   -- Tiger Palm
    { 100784, 574575 },   -- Blackout Kick
    { 107428, 642415, { 8 } },  -- Rising Sun Kick
    { 113656, 627606 },   -- Fists of Fury
    { 101546, 606543 },   -- Spinning Crane Kick
    { 152175, 988194 },   -- Whirling Dragon Punch
    { 322109, 606552 },   -- Touch of Death
    { 116841, 651727 },   -- Tiger's Lust
    { 119381, 642414 },   -- Leg Sweep
    { 115203, 615341 },   -- Fortifying Brew
    { 116670, 1360980 },  -- Vivify
}
for slot, spell in ipairs(spells) do
    local cooldown = spell[3] and { GetTime(), spell[3][1] } or nil
    S.SetAction(slot, { type = "spell", id = spell[1], texture = spell[2], cooldown = cooldown })
end

-- Tell the UI about the target and the group once it has loaded.
local f = CreateFrame("Frame")
f:RegisterEvent("PLAYER_ENTERING_WORLD")
f:SetScript("OnEvent", function(self)
    self:UnregisterAllEvents()
    S.FireEvent("GROUP_ROSTER_UPDATE")
    S.FireEvent("PLAYER_TARGET_CHANGED")
    S.FireEvent("ACTIONBAR_SLOT_CHANGED", 0)
end)
