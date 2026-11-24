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
          castTime = 2, cost = 2000, range = 40, harmful = true,
          description = "Hurls a lance of embers at the enemy." },
        { id = 1900002, name = "Frostbind", icon = icons .. "Spell_Frost_FrostNova",
          cooldown = 12, cost = 1500, range = 40,
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
          description = "Calls down falling stars on the enemy for 3 sec." },
        { id = 1900005, name = "Blinkstep", icon = icons .. "Spell_Arcane_Blink",
          cooldown = 15, gcd = 0,
          description = "Steps forward through the weave. Not on the global cooldown." },
        { id = 1900006, name = "Wellspring", icon = icons .. "Spell_Nature_ManaRegenTotem",
          cooldown = 45, restore = 30000,
          description = "Draws on the ley lines to restore 30,000 mana." },
        { id = 1900007, name = "Weave Attunement", icon = icons .. "Spell_Arcane_MindMastery",
          passive = true, subtext = "Passive",
          description = "Your spells cost 5% less mana." },
    },
})

State.SetDefaultActions({
    [1] = 1900001, [2] = 1900002, [3] = 1900003, [4] = 1900004,
})

State.AddHiddenUnit("dummy", {
    name = "Straw Sentinel", level = -1, classification = "normal",
    creatureType = "Mechanical", health = 5000000, healthMax = 5000000,
    power = 0, powerMax = 0, reaction = 2, canAttack = true, isPlayer = false,
    guid = "Creature-0-0-0-0-1900900-0000000001",
})

print("Dummy scenario: Ilyndra and the Straw Sentinel")
