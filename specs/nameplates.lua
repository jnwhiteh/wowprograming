-- Nameplates and clicks in the world (scenarios/dummy): units near the
-- player get nameplates over their heads, a click on a nameplate or on a
-- unit targets it, and plates follow health, death and the camera.
-- The test nameplates_and_world_clicks runs this spec. It expects a fresh
-- session (the player where the scenario puts it).

Wait(0.2)
local function plateFor(name)
    for _, f in ipairs(C_NamePlate.GetNamePlates()) do
        if UnitName(f.namePlateUnitToken) == name then return f end
    end
end

-- Enemies have plates (nameplateShowEnemies); the friendly NPC and the
-- party do not (nameplateShowFriendlyNPCs and nameplateShowFriends off).
local dummy, rat = plateFor("Straw Sentinel"), plateFor("Tunnel Rat")
assert(dummy and rat, "no plates for the enemies")
Equal(plateFor("Warden Halric"), nil)
Equal(plateFor("Brannoc"), nil)
Equal(dummy.UnitFrame.name:GetText(), "Straw Sentinel")
Equal(rat.UnitFrame.healthBar:GetValue(), 30000)
Equal(C_NamePlate.GetNamePlateForUnit(rat.namePlateUnitToken), rat)
Equal(UnitIsUnit(rat.namePlateUnitToken, "nameplate2") or UnitIsUnit(rat.namePlateUnitToken, "nameplate1"), true)

-- The plate stands over the unit's head: the dummy is straight ahead
-- (north), so its plate is centered; the rat is to the right (east).
local cx = dummy:GetCenter()
Near(cx, GetScreenWidth() / 2, 1)
assert(rat:GetCenter() > cx, "the rat's plate is not to the right")

-- A click on the rat's plate targets it.
local hx, hy = rat.UnitFrame.healthBar:GetCenter()
local s = rat.UnitFrame.healthBar:GetEffectiveScale()
ClickAt(hx * s, hy * s)
Equal(UnitName("target"), "Tunnel Rat")
-- A click on the dummy in the world targets it. Pick a point of its
-- body that no frame (or nameplate) covers: frames take clicks first.
local function covered(x, y)
    local f = EnumerateFrames()
    while f do
        if f:IsVisible() and f:IsMouseEnabled() then
            local l, b, w, h = f:GetRect()
            local s = f:GetEffectiveScale()
            if l and x >= l * s and x <= (l + w) * s and y >= b * s and y <= (b + h) * s then return true end
        end
        f = EnumerateFrames(f)
    end
    for _, p in ipairs(C_NamePlate.GetNamePlates()) do
        local l, b, w, h = p:GetRect()
        if l and x >= l and x <= l + w and y >= b and y <= b + h then return true end
    end
end
local bx, by
for _, height in ipairs({ 0.3, 0.8, 1.2, 1.6, 2.0 }) do
    local x, y = WowProgrammingState.BodyOnScreen("dummy", height)
    if not covered(x, y) then bx, by = x, y break end
end
assert(bx, "every point of the dummy is under a frame")
ClickAt(bx, by)
Equal(UnitName("target"), "Straw Sentinel")
-- Sticky targeting (deselectOnClick 0 in the captured settings): a click
-- on empty ground (here the sky) keeps the target; with it off, the
-- target clears.
ClickAt(GetScreenWidth() * 0.35, GetScreenHeight() * 0.85)
Equal(UnitName("target"), "Straw Sentinel")
-- The option is "Sticky Targeting" (a negated setting of the CVar); set
-- it as the options window does. SetCVar("deselectOnClick", "1") is
-- turned back by Blizzard's settings panel (see client-runtime.md).
Settings.SetValue("deselectOnClick", false)
Equal(GetCVar("deselectOnClick"), "1")
ClickAt(GetScreenWidth() * 0.35, GetScreenHeight() * 0.85)
Equal(UnitExists("target"), false)
Settings.SetValue("deselectOnClick", true)

-- Damage shows on the plate; a dead unit loses its plate (unit removed).
local removed
local watcher = CreateFrame("Frame")
watcher:RegisterEvent("NAME_PLATE_UNIT_REMOVED")
watcher:SetScript("OnEvent", function(_, _, token) removed = token end)
Command("/target Tunnel Rat")
local token = rat.namePlateUnitToken
Press("1")
Wait(2.2)
Equal(rat.UnitFrame.healthBar:GetValue(), 12000)
Wait(1.5)
Press("1")
Wait(2.2)
Equal(UnitIsDead("target"), true)
Wait(0.1)
Equal(removed, token)
Equal(plateFor("Tunnel Rat"), nil)
-- Back after 10 s, with a plate again.
Wait(10)
assert(plateFor("Tunnel Rat"), "no plate after the respawn")

-- Turning around puts the dummy behind the camera: its plate hides but
-- the unit keeps it.
Hold("A", 1) -- turn left 180 degrees (pi radians a second)
Wait(0.1)
dummy = plateFor("Straw Sentinel")
Equal(dummy:IsShown(), false)
Equal(C_NamePlateManager.IsNamePlateUnitBehindCamera(dummy.namePlateUnitToken), true)
Hold("D", 1)
Wait(0.1)
Equal(dummy:IsShown(), true)

-- Walking away past nameplateMaxDistance (60 yards) removes the plates.
Hold("S", 15)
Wait(0.1)
Equal(#C_NamePlate.GetNamePlates(), 0)
