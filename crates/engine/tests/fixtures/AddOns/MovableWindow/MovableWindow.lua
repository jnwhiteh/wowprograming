-- The window code of BugSack (sack.lua), without its template and
-- contents. Created at load time, so the client restores its position.
local window = CreateFrame("Frame", "MovableWindowFrame", UIParent)
window:SetSize(500, 310)
window:SetFrameStrata("DIALOG")
window:SetPoint("CENTER")
window:SetMovable(true)
window:EnableMouse(true)
window:RegisterForDrag("LeftButton")
window:SetClampedToScreen(true)
window:SetScript("OnDragStart", window.StartMoving)
window:SetScript("OnDragStop", window.StopMovingOrSizing)
