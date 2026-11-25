-- Combat in the world (scenarios/dummy): Tab picks hostile units by
-- distance, spells damage and heal, a unit dies and comes back, the
-- combat log records it, and range comes from positions.
-- The test combat_in_the_world runs this spec.

local logged, errors, health = {}, {}, {}
local watcher = CreateFrame("Frame")
watcher:RegisterEvent("COMBAT_LOG_EVENT_UNFILTERED")
watcher:RegisterEvent("COMBAT_LOG_MESSAGE")
watcher:RegisterEvent("UI_ERROR_MESSAGE")
watcher:RegisterUnitEvent("UNIT_HEALTH", "target")
watcher:SetScript("OnEvent", function(_, event, ...)
    if event == "COMBAT_LOG_EVENT_UNFILTERED" then
        logged[#logged + 1] = { C_CombatLogSecure.GetCurrentEventInfo() }
    elseif event == "COMBAT_LOG_MESSAGE" then
        logged.text = (logged.text or "") .. (...) .. "\n"
    elseif event == "UI_ERROR_MESSAGE" then
        errors[#errors + 1] = select(2, ...)
    else
        health[#health + 1] = UnitHealth("target")
    end
end)
local function lastLog(sub)
    for i = #logged, 1, -1 do
        if logged[i][2] == sub then return logged[i] end
    end
end

-- Tab: the dummy (10 yards) first, then the rat (29 yards), then around.
Press("TAB")
Equal(UnitName("target"), "Straw Sentinel")
Near(WowProgrammingState.Distance("target"), 10, 0.01)
Press("TAB")
Equal(UnitName("target"), "Tunnel Rat")
Press("TAB")
Equal(UnitName("target"), "Straw Sentinel")
Press("TAB")
Equal(UnitName("target"), "Tunnel Rat")

-- Two Emberlances (18,000 fire damage each) kill the rat (30,000 health).
Equal(C_Spell.IsSpellInRange(1900001, "target"), true)
Press("1")
Wait(2.2)
Equal(UnitHealth("target"), 12000)
Equal(TargetFrame.TargetFrameContent.TargetFrameContentMain.HealthBarsContainer.HealthBar:GetValue(), 12000)
local hit = lastLog("SPELL_DAMAGE")
Equal(hit[5], "Ilyndra")
Equal(hit[9], "Tunnel Rat")
Equal(hit[13], "Emberlance")
Equal(hit[15], 18000)
Equal(hit[16], -1, "overkill is -1 while the unit lives")
Press("1")
Wait(2.2)
Equal(UnitIsDead("target"), true)
Equal(UnitHealth("target"), 0)
Equal(lastLog("SPELL_DAMAGE")[16], 6000, "overkill")
assert(lastLog("PARTY_KILL"), "no PARTY_KILL")
assert(lastLog("UNIT_DIED"), "no UNIT_DIED")
Equal(health[#health], 0, "UNIT_HEALTH for the target")
-- Blizzard's combat log tab shows the hits (COMBAT_LOG_EVENT, then
-- COMBAT_LOG_MESSAGE from C_CombatLogSecure.CreateCombatLogMessage).
assert(logged.text and logged.text:find("Emberlance", 1, true), "no combat log text")
assert(logged.text:find("Fire", 1, true), "no school name")
Equal(COMBATLOG:GetNumMessages() >= 3, true)

-- A dead target: spells fail, Tab skips it.
Press("1")
Wait(0.1)
Equal(UnitCastingInfo("player"), nil)
Press("TAB")
Equal(UnitName("target"), "Straw Sentinel")

-- The rat comes back at full health 10 s after it died.
Wait(10)
Command("/target Tunnel Rat")
Equal(UnitIsDead("target"), false)
Equal(UnitHealth("target"), 30000)

-- Harmful spells need a hostile target; heals go to a friendly one.
Command("/target Warden Halric")
Press("1")
Wait(0.1)
Equal(UnitCastingInfo("player"), nil, "Emberlance at a friendly unit")
Press("7")
Wait(1.7)
Equal(UnitHealth("target"), 490000)
Equal(lastLog("SPELL_HEAL")[15], 40000)
Wait(1.5) -- the global cooldown

-- Healing stops at the maximum: Brannoc has 520,000 of 600,000.
Command("/target Brannoc")
Press("7")
Wait(1.7)
Equal(UnitHealth("party1"), 560000)
Press("7")
Wait(1.7)
Press("7")
Wait(1.7)
Equal(UnitHealth("party1"), 600000)
Equal(lastLog("SPELL_HEAL")[16], 40000, "overhealing")
Wait(1.5)
-- A heal over time ticks every 2 s (UnitHealth lowered first).
WowProgrammingState.Damage("party1", 10000)
Press("8")
Wait(4.2)
Equal(UnitHealth("party1"), 594000)
Equal(lastLog("SPELL_PERIODIC_HEAL")[13], "Renewing Bloom")

-- Range: walking backward from the rat (29 yards) takes it out of
-- Emberlance's 40 yards; the action button's hot key turns red and a
-- cast fails with the error.
Command("/target Tunnel Rat")
Wait(0.1)
Equal(ActionButton1.HotKey:GetVertexColor() < 1, true)
Hold("S", 4)
Wait(0.1)
assert(WowProgrammingState.Distance("target") > 40, "still in range")
Equal(C_Spell.IsSpellInRange(1900001, "target"), false)
Equal(C_ActionBar.IsActionInRange(1), false)
local r, g, b = ActionButton1.HotKey:GetVertexColor()
Near(r, RED_FONT_COLOR.r, 0.01)
Near(g, RED_FONT_COLOR.g, 0.01)
Press("1")
Wait(0.1)
Equal(UnitCastingInfo("player"), nil)
Equal(errors[#errors], SPELL_FAILED_OUT_OF_RANGE)
-- Tab finds nothing out of range: the dummy is now 26 yards away, the
-- rat 45.
Press("TAB")
Equal(UnitName("target"), "Straw Sentinel")

-- Damage and heals on the player: the hit number on the player frame
-- (UNIT_COMBAT) and floating combat text (COMBAT_TEXT_UPDATE), which
-- loads when its setting is turned on.
SetCVar("enableFloatingCombatText", "1")
Wait(0.1)
assert(CombatText, "Blizzard_CombatText did not load")
WowProgrammingState.Damage("player", 5000)
Wait(0.1)
Equal(PlayerFrame.feedbackText:GetText(), "5,000")
local function combatTexts()
    local out = {}
    for _, fs in CombatText:EnumerateActiveFontStrings() do out[#out + 1] = fs:GetText() end
    return table.concat(out, " ")
end
assert(combatTexts():find("-5,000", 1, true), "combat text: " .. combatTexts())
Command("/target Ilyndra")
Press("7")
Wait(1.7)
Equal(PlayerFrame.feedbackText:GetText(), "40,000")
assert(combatTexts():find("+40,000", 1, true), "combat text: " .. combatTexts())

-- The rat fights back: hurt, it runs to melee range and bites every 2 s.
SetCVar("enableFloatingCombatText", "0")
Command("/target Tunnel Rat")
Hold("W", 3) -- back within 40 yards
local hp = UnitHealth("player")
Press("2") -- Frostbind: instant
Wait(0.1)
Equal(WowProgrammingState.Engaged("target"), true)
Wait(8)
assert(WowProgrammingState.Distance("target") <= 5.5, "the rat did not come to melee range")
assert(UnitHealth("player") < hp, "the rat did not bite")
Equal(lastLog("SWING_DAMAGE")[5], "Tunnel Rat")
Equal(lastLog("SWING_DAMAGE")[12], 3000)
Equal(InCombatLockdown(), true)

-- The player dies: the release dialog, then a ghost at the graveyard,
-- back to the corpse, and up again at half health.
WowProgrammingState.Damage("player", UnitHealth("player"))
Wait(0.2)
Equal(UnitIsDead("player"), true)
Equal(WowProgrammingState.Engaged("target"), false)
Equal(StaticPopup_Visible("DEATH") ~= nil, true)
Press("1")
Wait(0.1)
Equal(UnitCastingInfo("player"), nil, "no casting while dead")
local corpseX, corpseY = UnitPosition("player")
StaticPopup_FindVisible("DEATH"):GetButton1():Click()
Wait(0.2)
Equal(UnitIsGhost("player"), true)
Equal(UnitIsDead("player"), false)
Equal(StaticPopup_Visible("DEATH"), nil)
local x = UnitPosition("player")
Near(x, corpseX - 60, 0.01, "released 60 yards south")
assert(C_DeathInfo.GetCorpseMapPosition(C_Map.GetBestMapForUnit("player")), "no corpse position")
Hold("W", 4) -- 28 yards north: 32 from the corpse
Wait(0.2)
Equal(StaticPopup_Visible("RECOVER_CORPSE") ~= nil, true)
StaticPopup_FindVisible("RECOVER_CORPSE"):GetButton1():Click()
Wait(0.2)
Equal(UnitIsGhost("player"), false)
Equal(UnitHealth("player"), UnitHealthMax("player") / 2)
