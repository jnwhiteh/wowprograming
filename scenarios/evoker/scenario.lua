-- Evoker scenario: replays API answers captured in game.
--
-- capture.txt comes from tools/capture/spellbook_capture.lua and
-- client_constants.txt from tools/capture/client_constants_capture.lua.
local dir, S = ...

local constants = S.LoadCapture("client_constants.txt")
if constants then
    S.ApplyClientConstants(constants)
else
    print("Evoker scenario: client_constants.txt is missing")
end

local capture = S.LoadCapture("capture.txt")
if capture then
    local n = S.Replay(capture.calls)
    print(string.format("Evoker scenario: replaying %d captured functions", n))
    WowProgrammingCapture = capture
else
    print("Evoker scenario: capture.txt is missing")
end

-- Spell data is always loaded in the sandbox.
S.Set("C_Spell.IsSpellDataCached", function() return true end)
S.Set("C_Spell.RequestLoadSpellData", function() end)
S.Set("UnitName", function(unit) return "Evoker", nil end)
