-- Monk scenario: replays API answers captured in game.
--
-- capture.txt comes from tools/capture/spellbook_capture.lua and
-- client_constants.txt from tools/capture/client_constants_capture.lua.
local dir, S = ...

local constants = S.LoadCapture("client_constants.txt")
if constants then
    S.ApplyClientConstants(constants)
else
    print("Monk scenario: client_constants.txt is missing")
end

local capture = S.LoadCapture("capture.txt")
if capture then
    local n = S.Replay(capture.calls)
    print(string.format("Monk scenario: replaying %d captured functions", n))
    WowProgrammingCapture = capture
else
    print("Monk scenario: capture.txt is missing")
end

-- Spell data is always loaded in the sandbox.
S.Set("C_Spell.IsSpellDataCached", function() return true end)
S.Set("C_Spell.RequestLoadSpellData", function() end)
S.Set("UnitName", function(unit) return "Monk", nil end)

-- GetSpecializationRoleEnum is not captured yet; derive it from the
-- captured specialization roles.
local roleEnum = { TANK = Enum.LFGRole.Tank, HEALER = Enum.LFGRole.Healer, DAMAGER = Enum.LFGRole.Damage }
S.Set("GetSpecializationRoleEnum", function(index, ...)
    local role = select(5, GetSpecializationInfo(index, ...))
    return roleEnum[role]
end)

S.Set("C_PetJournal.GetNumPetsNeedingFanfare", function() return 0 end)
S.Set("StoreFrame_IsShown", function() return false end)

-- The player's action bars. The real bars come from Blizzard_ActionBar,
-- which pulls in most of the game UI; empty stand-ins are enough for the
-- spellbook's "is this spell on a bar" checks.
for _, name in ipairs({ "MainActionBar", "MultiBarBottomLeft", "MultiBarBottomRight",
        "MultiBarRight", "MultiBarLeft", "MultiBar5", "MultiBar6", "MultiBar7" }) do
    local bar = CreateFrame("Frame", name, UIParent)
    bar.SetShowGrid = function() end
end
