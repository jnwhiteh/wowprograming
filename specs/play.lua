-- Sixty seconds of play in scenarios/dummy: walking, turning, Tab,
-- casting at enemies and friends, clicking nameplates, a fight with the
-- rat, and panels opened between. `wowprogramming test` (and the test
-- sixty_seconds_of_play) fails on any Lua error the UI reports.
-- It expects a fresh session.

local start = GetTime()
local function elapsed() return GetTime() - start end

local function clickPlate()
    local plate = C_NamePlate.GetNamePlates()[1]
    if plate and plate:IsShown() and plate.UnitFrame then
        local x, y = plate.UnitFrame.healthBar:GetCenter()
        local s = plate.UnitFrame.healthBar:GetEffectiveScale()
        ClickAt(x * s, y * s)
    end
end

-- One round is about 15 seconds; four rounds make a minute.
local round = 0
while elapsed() < 60 do
    round = round + 1
    Press("TAB")
    Press("1") -- Emberlance, 2 s
    Wait(2.2)
    Hold("W", 1.5)
    Press("2") -- Frostbind
    Wait(1.6)
    Press("4") -- Starfall Torrent, channelled 3 s
    Hold("A", 0.5)
    Wait(3)
    clickPlate()
    Press("3") -- Aegis of Dawn on the player
    Wait(1.6)
    Command("/target Brannoc")
    Press("8") -- Renewing Bloom
    Wait(1.6)
    Press("7") -- Mending Light
    Wait(1.7)
    Press("P") -- the spellbook, then close it
    Wait(0.3)
    Press("ESCAPE")
    Hold("D", 0.5)
    Hold("S", 1)
    Press("ESCAPE") -- clear the target
end

assert(round >= 4, "fewer rounds than expected: " .. round)
-- The player is still alive (the rat bites for 3,000 every 2 s).
Equal(UnitIsDeadOrGhost("player"), false)
