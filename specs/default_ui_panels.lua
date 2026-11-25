-- Opens every panel of the default UI, one at a time, and reports the Lua
-- errors each one raises. Fails if any panel raises an error.
-- wowprogramming test --scenario scenarios/dummy specs/default_ui_panels.lua
local current, byPanel, order = nil, {}, {}
local previous = geterrorhandler()
seterrorhandler(function(msg, ...)
    local key = current or "(between panels)"
    if not byPanel[key] then byPanel[key] = {}; order[#order + 1] = key end
    local list = byPanel[key]
    local first = tostring(msg):gsub("\n.*", "")
    local stack = debugstack(3, 3, 0) or ""
    first = first .. "\n      " .. stack:gsub("\n", "\n      ")
    local seen = false
    for _, m in ipairs(list) do if m == first then seen = true end end
    if not seen then list[#list + 1] = first end
    return previous(msg, ...)
end)

-- Panels that cannot work yet, with the reason; printed, not counted.
local known = {
    Talents = "needs specialization and talent tree (C_Traits) data",
}

local panels = {
    { "Character", function() ToggleCharacter("PaperDollFrame") end, "CharacterFrame" },
    { "Reputation", function() ToggleCharacter("ReputationFrame") end, "CharacterFrame" },
    { "Currency", function() ToggleCharacter("TokenFrame") end, "CharacterFrame" },
    { "Spellbook", function() PlayerSpellsUtil.ToggleSpellBookFrame() end, "PlayerSpellsFrame" },
    { "Talents", function() PlayerSpellsUtil.ToggleClassTalentOrSpecFrame() end, "PlayerSpellsFrame" },
    { "Professions book", function() ToggleProfessionsBook() end, "ProfessionsBookFrame" },
    { "Achievements", function() ToggleAchievementFrame() end, "AchievementFrame" },
    { "Quest log", function() ToggleQuestLog() end, "WorldMapFrame" },
    { "World map", function() ToggleWorldMap() end, "WorldMapFrame" },
    { "Guild and communities", function() ToggleGuildFrame() end, "CommunitiesFrame" },
    { "Communities", function() ToggleCommunitiesFrame() end, "CommunitiesFrame" },
    { "Group finder", function() PVEFrame_ToggleFrame("GroupFinderFrame") end, "PVEFrame" },
    { "Premade groups", function() PVEFrame_ToggleFrame("GroupFinderFrame", LFGListPVEStub) end, "PVEFrame" },
    { "PvP", function() PVEFrame_ToggleFrame("PVPUIFrame") end, "PVEFrame" },
    { "Adventure guide", function() ToggleEncounterJournal() end, "EncounterJournal" },
    { "Mounts", function() ToggleCollectionsJournal(1) end, "CollectionsJournal" },
    { "Pets", function() ToggleCollectionsJournal(2) end, "CollectionsJournal" },
    { "Toys", function() ToggleCollectionsJournal(3) end, "CollectionsJournal" },
    { "Heirlooms", function() ToggleCollectionsJournal(4) end, "CollectionsJournal" },
    { "Appearances", function() ToggleCollectionsJournal(5) end, "CollectionsJournal" },
    { "Calendar", function() ToggleCalendar() end, "CalendarFrame" },
    { "Friends", function() ToggleFriendsFrame(1) end, "FriendsFrame" },
    { "Who", function() ToggleFriendsFrame(2) end, "FriendsFrame" },
    { "Raid", function() ToggleFriendsFrame(3) end, "FriendsFrame" },
    { "Bags", function() ToggleAllBags() end, "ContainerFrameCombinedBags|ContainerFrame1" },
    { "Game menu", function() ToggleGameMenu() end, "GameMenuFrame" },
    { "Settings", function() Settings.OpenToCategory(Settings.GAMEPLAY_CATEGORY_ID or 1) end, "SettingsPanel" },
    { "Key bindings", function() Settings.OpenToCategory(Settings.KEYBINDINGS_CATEGORY_ID) end, "SettingsPanel" },
    { "Macros", function() ShowMacroFrame() end, "MacroFrame" },
    { "Help", function() ToggleHelpFrame() end, "HelpFrame" },
    { "Edit Mode", function() ShowUIPanel(EditModeManagerFrame) end, "EditModeManagerFrame" },
    { "Dressing room", function() DressUpFrame_Show(DressUpFrame) end, "DressUpFrame" },
    { "Mail", function() ShowUIPanel(MailFrame) end, "MailFrame" },
    { "Housing", function() if HousingFramesUtil then HousingFramesUtil.ToggleHousingDashboard() end end },
}

for _, p in ipairs(panels) do
    current = p[1]
    local ok, err = pcall(p[2])
    if not ok then
        if not byPanel[current] then byPanel[current] = {}; order[#order + 1] = current end
        table.insert(byPanel[current], "opening failed: " .. tostring(err):gsub("\n.*", ""))
    end
    Wait(0.5)
    -- p[3] names the frame that shows; "A|B" accepts either.
    local shownOk = not p[3]
    for name in (p[3] or ""):gmatch("[^|]+") do
        if _G[name] and _G[name]:IsShown() then shownOk = true end
    end
    if not shownOk then
        if not byPanel[current] then byPanel[current] = {}; order[#order + 1] = current end
        table.insert(byPanel[current], p[3] .. " did not show")
    end
    if EditModeManagerFrame and EditModeManagerFrame:IsShown() then HideUIPanel(EditModeManagerFrame) end
    CloseAllWindows()
    if GameMenuFrame and GameMenuFrame:IsShown() then HideUIPanel(GameMenuFrame) end
    if SettingsPanel and SettingsPanel:IsShown() then HideUIPanel(SettingsPanel) end
    Wait(0.3)
    current = nil
end

local count = 0
for _, key in ipairs(order) do
    if known[key] then
        print(key .. " (known gap: " .. known[key] .. "):")
        for _, m in ipairs(byPanel[key]) do print("  " .. m) end
    else
        print(key .. ":")
        for _, m in ipairs(byPanel[key]) do print("  " .. m); count = count + 1 end
    end
end
print(count .. " distinct errors in " .. #order .. " places")
assert(count == 0, count .. " problems (listed above)")
