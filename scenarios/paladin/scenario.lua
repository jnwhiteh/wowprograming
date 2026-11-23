-- Paladin scenario: replays API answers captured in game.
--
-- capture.txt comes from the WowProgrammingCapture addon
-- (tools/capture/WowProgrammingCapture, imported with
-- `wowprogramming import-capture`).
local dir, S = ...

S.LoadStandardCapture("Paladin")
-- With --full-ui the real action bars and settings panel load.
if not S.fullUI then
    S.SpellbookStandIns()
end
