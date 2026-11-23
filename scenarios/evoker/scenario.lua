-- Evoker scenario: replays API answers captured in game.
--
-- capture.txt comes from the WowProgrammingCapture addon
-- (tools/capture/WowProgrammingCapture, imported with
-- `wowprogramming import-capture`). Older captures came from
-- tools/capture/spellbook_capture.lua, with client_constants.txt from
-- tools/capture/client_constants_capture.lua.
local dir, S = ...

local capture = S.LoadStandardCapture("Evoker")
-- With --full-ui the real action bars and settings panel load.
if not S.fullUI then
    S.SpellbookStandIns()
end

if not S.replayed["UnitName"] then
    S.Set("UnitName", function(unit) return "Evoker", nil end)
end

-- Older captures show the Talents tab but did not record these calls.
if capture and not S.replayed["C_SpecializationInfo.CanPlayerUseTalentUI"] then
    S.Set("C_SpecializationInfo.CanPlayerUseTalentUI", function() return true end)
end
if capture and not S.replayed["IsPlayerInitialSpec"] then
    S.Set("IsPlayerInitialSpec", function() return false end)
end
