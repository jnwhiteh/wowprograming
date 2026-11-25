-- Answers for client functions that the API documentation does not
-- describe. Generated stubs return nothing for them, and Blizzard code
-- then compares nil with a number or iterates nil. Each answer is what an
-- empty character (no achievements, no friends, no pets) gets, inferred
-- from how Blizzard code uses the result.
local type, pairs, ipairs, select = type, pairs, ipairs, select

-- path -> function. Paths name globals ("GetAverageItemLevel") or
-- namespace functions ("C_PetJournal.GetNumPets").
local answers = {
    -- Character: average item level overall, equipped, PvP.
    GetAverageItemLevel = function() return 0, 0, 0 end,
    -- Achievements: no categories, no criteria.
    GetCategoryList = function() return {} end,
    GetGuildCategoryList = function() return {} end,
    GetStatisticsCategoryList = function() return {} end,
    GetAchievementNumCriteria = function() return 0 end,
    -- Any achievement: not completed, 10 points. A placeholder name until
    -- the Achievement table is read: id, name, points, completed, month,
    -- day, year, description, flags, icon, rewardText, isGuild,
    -- wasEarnedByMe, earnedBy, isStatistic.
    GetAchievementInfo = function(id, index)
        if index then return nil end
        if type(id) ~= "number" then return nil end
        return id, "Achievement " .. id, 10, false, nil, nil, nil, "", 0, 136243, "",
            false, false, nil, false
    end,
    GetNumCompletedAchievements = function() return 0, 0 end,
    GetTotalAchievementPoints = function() return 0 end,
    GetLatestCompletedAchievements = function() end,
    GetLatestUpdatedStats = function() end,
    GetCategoryNumAchievements = function() return 0, 0, 0 end,
    -- Encounter journal: tier 1 selected, no loot filter.
    EJ_GetCurrentTier = function() return 1 end,
    EJ_GetNumTiers = function() return 1 end,
    EJ_GetLootFilter = function() return 0, 0 end,
    EJ_GetSlotFilter = function() return 0 end,
    EJ_GetDifficulty = function() return 1 end,
    -- Pets and friends.
    ["C_PetJournal.GetNumPets"] = function() return 0, 0 end,
    ["C_PetJournal.GetNumCollectedInfo"] = function() return 0, 0 end,
    BNGetNumFriendInvites = function() return 0 end,
    BNGetNumFriends = function() return 0, 0, 0, 0 end,
    -- Character stats.
    GetCombatRatingBonus = function() return 0 end,
    GetCombatRating = function() return 0 end,
    -- Encounter journal loot.
    EJ_GetNumLoot = function() return 0 end,
    -- Group roles: the party's assigned roles (UnitGroupRolesAssigned).
    GetGroupMemberCounts = function()
        local counts = { TANK = 0, HEALER = 0, DAMAGER = 0, NOROLE = 0 }
        local units = { "player" }
        for i = 1, 4 do units[#units + 1] = "party" .. i end
        for _, u in ipairs(units) do
            if UnitExists(u) then
                local role = UnitGroupRolesAssigned(u)
                counts[role] = (counts[role] or 0) + 1
            end
        end
        return counts
    end,
    -- Money in a trade window.
    GetPlayerTradeMoney = function() return 0 end,
    GetTargetTradeMoney = function() return 0 end,
    -- The shop needs Battle.net; it is never available in the sandbox.
    ["C_StorePublic.IsEnabled"] = function() return false end,
    ["C_StorePublic.IsDisabledByParentalControls"] = function() return false end,
}

local function set(path, fn)
    local ns, name = path:match("^([^.]+)%.(.+)$")
    if ns then
        local t = _G[ns]
        if type(t) ~= "table" then t = {}; _G[ns] = t end
        t[name] = fn
    else
        _G[path] = fn
    end
end

for path, fn in pairs(answers) do set(path, fn) end

-- The namespaced copy of GetInventorySlotInfo (client.lua).
C_PaperDollInfo = C_PaperDollInfo or {}
C_PaperDollInfo.GetInventorySlotInfo = function(...) return GetInventorySlotInfo(...) end

-- Transmog slot locations: the client's fixed list of appearance and
-- illusion slots (Enum.TransmogOutfitSlot, TransmogType 0 appearance and
-- 1 illusion, TransmogCollectionType; weapons have no fixed collection).
C_TransmogOutfitInfo = C_TransmogOutfitInfo or {}
function C_TransmogOutfitInfo.GetAllSlotLocationInfo()
    local function slot(outfitSlot, kind, collection, name, secondary)
        return { slot = outfitSlot, type = kind, collectionType = collection,
                 slotName = name, isSecondary = secondary or false }
    end
    local appearance = {
        slot(0, 0, 1, "HEADSLOT"), slot(1, 0, 2, "SHOULDERSLOT"),
        slot(2, 0, 2, "SHOULDERSLOT", true), slot(3, 0, 3, "BACKSLOT"),
        slot(4, 0, 4, "CHESTSLOT"), slot(5, 0, 6, "TABARDSLOT"), slot(6, 0, 5, "SHIRTSLOT"),
        slot(7, 0, 7, "WRISTSLOT"), slot(8, 0, 8, "HANDSSLOT"), slot(9, 0, 9, "WAISTSLOT"),
        slot(10, 0, 10, "LEGSSLOT"), slot(11, 0, 11, "FEETSLOT"),
        slot(12, 0, 0, "MAINHANDSLOT"), slot(13, 0, 0, "SECONDARYHANDSLOT"),
    }
    local illusion = { slot(12, 1, 0, "MAINHANDSLOT"), slot(13, 1, 0, "SECONDARYHANDSLOT") }
    return appearance, illusion
end
