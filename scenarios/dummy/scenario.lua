-- Training dummy session. The character, the spells and the dummy are
-- invented; only the icons come from the game. Client constants (enums,
-- CVars, atlases) come from the Evoker capture, as in scenarios/world.
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
local names = capture and capture.meta and capture.meta.localizedClasses or {}

S.SetUnit("player", {
    name = "Ilyndra", level = 80,
    class = names.MAGE or "Mage", classFile = "MAGE", classID = 8,
    race = "Night Elf", raceFile = "NightElf", raceID = 4, sex = 3,
    health = 412000, healthMax = 412000,
    power = 100000, powerMax = 100000, powerType = 0, powerToken = "MANA",
    isPlayer = true, reaction = 5, faction = "Alliance", role = "DAMAGER",
    guid = "Player-1-0000DUMMY",
})

local State = S.State
local icons = "Interface\\Icons\\"

State.AddSkillLine({
    name = "General", icon = icons .. "INV_Misc_Book_09",
    spells = {
        { id = 1900101, name = "Kindle Campfire", icon = icons .. "Spell_Fire_Fire",
          castTime = 2.5, cooldown = 60, subtext = "",
          description = "Builds a small fire that warms nearby allies." },
        { id = 1900102, name = "Wayfarer's Return", icon = icons .. "INV_Misc_Rune_01",
          castTime = 10, cooldown = 900,
          description = "Returns you to the last inn where you rested." },
    },
})

State.AddSkillLine({
    name = "Spellweaving", icon = icons .. "Spell_Arcane_ArcaneTorrent",
    spells = {
        { id = 1900001, name = "Emberlance", icon = icons .. "Spell_Fire_FlameBolt",
          castTime = 2, cost = 2000, range = 40, harmful = true, damage = 18000, school = "fire",
          description = "Hurls a lance of embers at the enemy." },
        { id = 1900002, name = "Frostbind", icon = icons .. "Spell_Frost_FrostNova",
          cooldown = 12, cost = 1500, range = 40, damage = 6000, school = "frost",
          aura = { harmful = true, duration = 8, dispelName = "Magic",
                   description = "Movement slowed by 50%." },
          description = "Binds the enemy in frost, slowing it by 50% for 8 sec." },
        { id = 1900003, name = "Aegis of Dawn", icon = icons .. "Spell_Holy_PowerWordShield",
          cost = 1000,
          aura = { duration = 30, maxStacks = 3,
                   description = "Absorbs damage. Stacks up to 3 times." },
          description = "Wraps you in dawnlight that absorbs damage for 30 sec. Stacks up to 3 times." },
        { id = 1900004, name = "Starfall Torrent", icon = icons .. "Spell_Arcane_StarFire",
          channel = 3, cooldown = 20, cost = 4000, range = 40, harmful = true,
          tickDamage = 9000, school = "arcane",
          description = "Calls down falling stars on the enemy for 3 sec." },
        { id = 1900005, name = "Blinkstep", icon = icons .. "Spell_Arcane_Blink",
          cooldown = 15, gcd = 0,
          description = "Steps forward through the weave. Not on the global cooldown." },
        { id = 1900006, name = "Wellspring", icon = icons .. "Spell_Nature_ManaRegenTotem",
          cooldown = 45, restore = 30000,
          description = "Draws on the ley lines to restore 30,000 mana." },
        { id = 1900008, name = "Renewing Bloom", icon = icons .. "Spell_Nature_Rejuvenation",
          cost = 1500, range = 40, school = "nature",
          aura = { duration = 12, self = false, tick = 2, heal = 2000,
                   description = "Heals 2,000 health every 2 sec." },
          description = "Heals a friendly target for 12,000 over 12 sec." },
        { id = 1900009, name = "Mending Light", icon = icons .. "Spell_Holy_FlashHeal",
          castTime = 1.5, cost = 2500, range = 40, heal = 40000, school = "holy",
          description = "Heals a friendly target for 40,000." },
        { id = 1900007, name = "Weave Attunement", icon = icons .. "Spell_Arcane_MindMastery",
          passive = true, subtext = "Passive",
          description = "Your spells cost 5% less mana." },
    },
})

-- A macro: Frostbind on a hostile target, then Blinkstep (off the
-- global cooldown, so both go off from one press).
State.AddMacro({
    name = "Bind & Blink", perCharacter = true,
    body = "#showtooltip Frostbind\n/cast [@target,harm] Frostbind\n/cast Blinkstep",
})

State.SetDefaultActions({
    [1] = 1900001, [2] = 1900002, [3] = 1900003, [4] = 1900004,
    [6] = { macro = "Bind & Blink" }, [7] = 1900009, [8] = 1900008,
})

-- Ilyndra stands in Goldshire, Elwynn Forest (UiMap 37), facing north.
WowProgrammingWorld.SetPlayerMapPosition(37, 0.42, 0.65)
local me = WowProgrammingWorld.Player()
-- Other units stand at offsets from the player, in yards (north, west).
local function near(north, west) return me.x + north, me.y + west end

-- Two friends in the party, for click casting on party frames.
local party = {
    { name = "Brannoc", class = "Warrior", classFile = "WARRIOR", classID = 1,
      race = "Dwarf", raceFile = "Dwarf", raceID = 3, role = "TANK", powerType = 1,
      powerToken = "RAGE", health = 520000, healthMax = 600000, power = 20, powerMax = 100 },
    { name = "Seralyn", class = "Priest", classFile = "PRIEST", classID = 5,
      race = "Human", raceFile = "Human", raceID = 1, role = "HEALER", powerType = 0,
      powerToken = "MANA", health = 380000, healthMax = 380000, power = 250000, powerMax = 250000 },
}
for i, member in ipairs(party) do
    member.level, member.isPlayer, member.reaction = 80, true, 5
    member.faction, member.inParty = "Alliance", true
    member.guid = "Player-1-0000PRTY" .. i
    -- They walk along with Ilyndra, 3 yards behind her.
    member.follow = { -3, i == 1 and 2 or -2 }
    S.SetUnit("party" .. i, member)
end
S.units.player.inParty = true
S.Set("IsInGroup", function() return true end)
S.Set("IsInRaid", function() return false end)
S.Set("GetNumGroupMembers", function() return #party + 1 end)
S.Set("GetNumSubgroupMembers", function() return #party end)
S.Set("UnitIsGroupLeader", function(unit) return unit == "player" end)

State.AddHiddenUnit("dummy", {
    name = "Straw Sentinel", level = -1, classification = "normal",
    creatureType = "Mechanical", health = 5000000, healthMax = 5000000,
    power = 0, powerMax = 0, reaction = 2, canAttack = true, isPlayer = false,
    guid = "Creature-0-0-0-0-1900900-0000000001",
    -- It cannot be killed for good: at 0 health it is back 3 s later.
    respawn = 3,
})

-- A hostile creature that dies and comes back after 10 s.
State.AddHiddenUnit("rat", {
    name = "Tunnel Rat", level = 78, classification = "normal",
    creatureType = "Beast", health = 30000, healthMax = 30000,
    power = 0, powerMax = 0, reaction = 2, canAttack = true, isPlayer = false,
    guid = "Creature-0-0-0-0-1900901-0000000001", respawn = 10,
})

-- A friendly guard: helpful spells can target it, harmful ones cannot.
State.AddHiddenUnit("warden", {
    name = "Warden Halric", level = 80, classification = "normal",
    creatureType = "Humanoid", health = 450000, healthMax = 500000,
    power = 0, powerMax = 0, reaction = 5, canAttack = false, isPlayer = false,
    guid = "Creature-0-0-0-0-1900902-0000000001",
})

State.PlaceUnit("dummy", near(10, 0))
State.PlaceUnit("rat", near(25, -15))
State.PlaceUnit("warden", near(6, 8))

-- A curse from someone else, so the debuff frame shows a dispellable
-- debuff with its type color.
State.ApplyAura("player", 1900801, {
    name = "Hex of Brittle Bones", icon = icons .. "Spell_Shadow_GrimWard",
    harmful = true, dispelName = "Magic", duration = 1800, source = "other",
    description = "Armor reduced by 10%.",
})


print("Dummy scenario: Ilyndra and the Straw Sentinel")
