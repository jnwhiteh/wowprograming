-- Client state: what the C++ client knows about the player and answers to
-- the UI. Spells and the spellbook, action slots, the cursor, the target,
-- casting, cooldowns, power and auras, as one model.
--
-- A scenario turns the model on by describing a character with S.State
-- (see the functions at the end of this file). Scenarios that replay a
-- capture do not call it, and keep their captured answers.
--
-- Rules marked "inferred" come from Blizzard code or the API
-- documentation, not from the real client. See docs/client-runtime.md.
local S, fireNow = ...
local type, pairs, ipairs, select, tostring, tonumber, unpack, math, table, string =
      type, pairs, ipairs, select, tostring, tonumber, unpack, math, table, string

local State = {}
S.State = State
-- Specs and tests reach the model through this global.
WowProgrammingState = State

local spells = {}      -- spell ID -> spell
local spellByName = {} -- lower-case name -> spell
local skillLines = {}  -- { name, icon, spells = { spell IDs } }
local book = {}        -- spellbook slot -> spell ID (built from skillLines)
local units = S.units  -- shared with S.SetUnit
local hidden = {}      -- units without a token until targeted (the dummy)
local actions          -- slot -> { type, id }; saved for the character
local cursor           -- what the player holds: { type = "spell"|"action", ... }
local cast             -- the cast in progress
local gcdStart, gcdDuration = 0, 0
local cooldowns = {}   -- spell ID -> { start, duration }
local auras = {}       -- unit data -> list of auras
local macros = {}      -- macro index -> { name, icon, body }
local nextAuraID, nextCastID = 1, 1
local inCombat, lockdown, lastHostile = false, false, 0
local COMBAT_TIMEOUT = 6 -- seconds without a hostile action (inferred)
local enabled = false

State.spells = spells

local GCD = 1.5
local SAVED = "WowProgramming_ActionSlots"

-- Events ----------------------------------------------------------------------

-- The client answers most requests after a server round trip, so events
-- are delivered on the next frame, in order (inferred).
local queue = {}
local flushing = false
local function flush()
    flushing = false
    local q = queue
    queue = {}
    for _, e in ipairs(q) do
        if e.fn then e.fn() else fireNow(unpack(e, 1, e.n)) end
    end
end
local function schedule()
    if not flushing then
        flushing = true
        C_Timer.After(0, flush)
    end
end
local function fire(...)
    queue[#queue + 1] = { n = select("#", ...), ... }
    schedule()
end
-- Run a function in event order (for events that set state first).
local function queueCall(fn)
    queue[#queue + 1] = { fn = fn }
    schedule()
end
State.Fire = fire

-- Helpers ---------------------------------------------------------------------

local function now() return GetTime() end

local function iconID(icon)
    if type(icon) == "string" then
        return GetFileIDFromPath(icon) or 134400
    end
    return icon or 134400 -- INV_Misc_QuestionMark
end

local function findSpell(id)
    if id == nil then return nil end
    local n = tonumber(id)
    if n then return spells[n] end
    if type(id) == "string" then
        -- "Name(Rank 1)" and "Name" both name a spell.
        return spellByName[id:gsub("%b()", ""):lower()]
    end
end

local function unitData(unit)
    if type(unit) ~= "string" then return nil end
    return units[unit:lower()]
end

local function player() return units.player end

local function powerToken(u) return u.powerToken or "MANA" end

local function setPower(u, value)
    value = math.max(0, math.min(u.powerMax or 100, value))
    if value == u.power then return end
    u.power = value
    fire("UNIT_POWER_UPDATE", u.token, powerToken(u))
    fire("UNIT_POWER_FREQUENT", u.token, powerToken(u))
end

local function cooldownInfo(start, duration)
    local active = duration > 0 and start + duration > now()
    if not active then start, duration = 0, 0 end
    return { startTime = start, duration = duration, isEnabled = true, modRate = 1,
        isActive = active, activeCategory = nil }
end

-- The cooldown a button shows for a spell: its own, or the global cooldown
-- if that ends later.
local function spellCooldown(s)
    local cd = cooldowns[s.id]
    local start, duration = 0, 0
    if cd and cd.start + cd.duration > now() then start, duration = cd.start, cd.duration end
    if s.gcd ~= 0 and gcdStart + gcdDuration > now() and gcdStart + gcdDuration > start + duration then
        start, duration = gcdStart, gcdDuration
    end
    local info = cooldownInfo(start, duration)
    info.isOnGCD = duration > 0 and start == gcdStart and duration == gcdDuration
    return info
end

local function powerCost(s)
    local u = player()
    if not s.cost or not u then return {} end
    return { {
        type = u.powerType or 0, name = powerToken(u), cost = s.cost, minCost = s.cost,
        costPercent = 0, costPerSec = 0, requiredAuraID = 0, hasRequiredAura = false,
    } }
end

local function usable(s)
    if not s or s.passive then return false, false end
    local u = player()
    if s.cost and u and (u.power or 0) < s.cost then return false, true end
    return true, false
end

local function link(s)
    return string.format("|cff71d5ff|Hspell:%d:0|h[%s]|h|r", s.id, s.name)
end

-- Spellbook ---------------------------------------------------------------------

local function rebuildBook()
    for k in pairs(book) do book[k] = nil end
    local slot = 0
    for _, line in ipairs(skillLines) do
        line.offset = slot
        for _, id in ipairs(line.spells) do
            slot = slot + 1
            book[slot] = id
        end
    end
end

local function bookSpell(slot, bank)
    if bank ~= nil and bank ~= Enum.SpellBookSpellBank.Player then return nil end
    return spells[book[slot]]
end

local function lineOf(slot)
    for i, line in ipairs(skillLines) do
        if slot > line.offset and slot <= line.offset + #line.spells then return i end
    end
end

-- Cursor ----------------------------------------------------------------------

local cursorTypes = { spell = 3, action = 6, macro = 7 } -- Enum.UICursorType

local function setCursor(new)
    local old = cursor
    cursor = new
    local oldType = old and cursorTypes[old.type] or 0
    local newType = new and cursorTypes[new.type] or 0
    -- Holding a spell or an action shows the empty action slots (inferred
    -- from ActionBarMixin, which raises the bars for these cursor types).
    if old and not new then fire("ACTIONBAR_HIDEGRID") end
    if new and not old then fire("ACTIONBAR_SHOWGRID") end
    fire("CURSOR_CHANGED", new == nil, newType, oldType, 0)
end

-- Action slots ----------------------------------------------------------------

-- Macros ----------------------------------------------------------------------

-- Character macros start at 121, after the 120 account-wide ones.
local MAX_ACCOUNT_MACROS = 120

local function findMacro(id)
    if type(id) == "number" then return macros[id], id end
    if type(id) == "string" then
        local n = tonumber(id)
        if n then return macros[n], n end
        for i, m in pairs(macros) do
            if m.name:lower() == id:lower() then return m, i end
        end
    end
end

-- The spell a macro shows: "#showtooltip Spell", else the first /cast
-- or /use whose options hold (inferred from how macros look in game).
local function macroSpell(m)
    if not m then return nil end
    local fallback
    for raw in (m.body .. "\n"):gmatch("([^\r\n]*)\r?\n") do
        local cmd, rest = raw:match("^%s*([#/]%S+)%s*(.-)%s*$")
        cmd = cmd and cmd:lower()
        if cmd == "#showtooltip" and rest ~= "" then
            local action = SecureCmdOptionParse(rest)
            return action and findSpell(action)
        elseif (cmd == "/cast" or cmd == "/use") and not fallback then
            local action = SecureCmdOptionParse(rest)
            fallback = action and findSpell(action)
        end
    end
    return fallback
end

local function macroIcon(m)
    if m.icon then return m.icon end
    local s = macroSpell(m)
    return s and s.icon or 134400
end

local function actionMacro(slot)
    local a = actions and actions[slot]
    if a and a.type == "macro" then return macros[a.id] end
end

-- The spell behind an action: the spell itself, or the one its macro shows.
local function actionSpell(slot)
    local a = actions and actions[slot]
    if not a then return nil end
    if a.type == "spell" then return spells[a.id] end
    if a.type == "macro" then return macroSpell(macros[a.id]) end
end

local function setAction(slot, a)
    actions[slot] = a
    _G[SAVED] = _G[SAVED] or {}
    _G[SAVED][slot] = a and { a.type, a.id } or false
    fire("ACTIONBAR_SLOT_CHANGED", slot)
end

-- Put what the cursor holds into a slot; what was there goes on the cursor.
local function placeAction(slot)
    if not cursor then return end
    local held = cursor
    local previous = actions[slot]
    local new = { type = "spell", id = held.id }
    if held.type == "macro" then new = { type = "macro", id = held.id } end
    setAction(slot, new)
    if previous and (held.type ~= "action" or held.slot ~= slot) then
        setCursor({ type = "action", slot = slot, id = previous.id, actionType = previous.type })
    else
        setCursor(nil)
    end
end

-- Target ----------------------------------------------------------------------

local function setTarget(u)
    if units.target == u then return end
    units.target = u
    fire("PLAYER_TARGET_CHANGED")
end

-- Find a unit by token, or by name among described and hidden units.
local function resolveUnit(name, exact)
    if name == nil then return nil end
    local u = unitData(name)
    if u then return u end
    local lower = tostring(name):lower()
    for _, list in ipairs({ units, hidden }) do
        for _, x in pairs(list) do
            if x.name and x.name:lower() == lower then return x end
        end
    end
    -- "/target Straw" finds the Straw Sentinel: names match by their start
    -- unless an exact match is asked for (inferred).
    if not exact then
        for _, list in ipairs({ units, hidden }) do
            for _, x in pairs(list) do
                if x.name and x.name:lower():sub(1, #lower) == lower then return x end
            end
        end
    end
end

-- Auras -----------------------------------------------------------------------

local function auraList(u)
    local list = auras[u]
    if not list then
        list = {}
        auras[u] = list
    end
    return list
end

-- UNIT_AURA goes to every token that names the unit.
local function fireAura(u, info)
    if not hidden[u.token] then fire("UNIT_AURA", u.token, info) end
    if units.target == u then fire("UNIT_AURA", "target", info) end
end

local function filterMatches(a, filter)
    filter = filter or "HELPFUL"
    if filter:find("HARMFUL") then
        if not a.isHarmful then return false end
    elseif not a.isHelpful then
        return false
    end
    if filter:find("PLAYER") and a.sourceUnit ~= "player" then return false end
    if filter:find("CANCELABLE") and a.isHarmful then return false end
    return true
end

local function filtered(u, filter)
    local out = {}
    for _, a in ipairs(auras[u] or {}) do
        if filterMatches(a, filter) then out[#out + 1] = a end
    end
    return out
end

local function auraData(a)
    local copy = {}
    for k, v in pairs(a) do copy[k] = v end
    copy.expires, copy.periodic = nil, nil
    return copy
end

local function applyAura(u, s, def)
    local list = auraList(u)
    local duration = def.duration or 0
    for _, a in ipairs(list) do
        if a.spellId == s.id and a.sourceUnit == "player" then
            -- Refresh, and add a stack up to the maximum (inferred).
            local max = def.maxStacks or 0
            if max > 0 then a.applications = math.min(max, math.max(a.applications, 1) + 1) end
            a.duration = duration
            a.expirationTime = duration > 0 and now() + duration or 0
            fireAura(u, { isFullUpdate = false, updatedAuraInstanceIDs = { a.auraInstanceID } })
            return a
        end
    end
    local harmful = def.harmful == true
    local a = {
        applications = (def.maxStacks or 0) > 0 and 1 or 0,
        auraInstanceID = nextAuraID,
        canActivePlayerDispel = def.dispelName ~= nil and not harmful,
        canApplyAura = true,
        dispelName = def.dispelName,
        duration = duration,
        expirationTime = duration > 0 and now() + duration or 0,
        icon = def.icon and iconID(def.icon) or s.icon,
        isBossAura = false,
        isFromPlayerOrPlayerPet = def.source ~= "other",
        isHarmful = harmful,
        isHelpful = not harmful,
        isNameplateOnly = false,
        isRaid = false,
        isStealable = false,
        name = def.name or s.name,
        nameplateShowAll = false,
        nameplateShowPersonal = false,
        points = def.points or {},
        sourceUnit = def.source ~= "other" and "player" or nil,
        spellId = s.id,
        timeMod = 1,
        description = def.description,
    }
    nextAuraID = nextAuraID + 1
    if def.tick and (def.heal or def.damage) then
        -- Not part of the aura data Blizzard code sees (auraData drops it).
        a.periodic = { interval = def.tick, next = now() + def.tick, heal = def.heal,
                       damage = def.damage, spell = s, source = def.source ~= "other" and player() or nil }
    end
    list[#list + 1] = a
    fireAura(u, { isFullUpdate = false, addedAuras = { auraData(a) } })
    return a
end

local function removeAuraImpl(u, instanceID)
    local list = auras[u] or {}
    for i, a in ipairs(list) do
        if a.auraInstanceID == instanceID then
            table.remove(list, i)
            fireAura(u, { isFullUpdate = false, removedAuraInstanceIDs = { instanceID } })
            return true
        end
    end
    return false
end

-- Combat ----------------------------------------------------------------------

-- Entering combat: PLAYER_REGEN_DISABLED arrives while protected frames
-- can still change; the lockdown starts after it (inferred from how
-- addons use the event). Leaving: the lockdown ends, then
-- PLAYER_REGEN_ENABLED.
local function enterCombat()
    lastHostile = GetTime()
    if inCombat then return end
    inCombat = true
    fire("PLAYER_REGEN_DISABLED")
    C_Timer.After(0, function() if inCombat then lockdown = true end end)
end

local function leaveCombat()
    if not inCombat then return end
    inCombat, lockdown = false, false
    fire("PLAYER_REGEN_ENABLED")
end

local removeAura = removeAuraImpl

-- Positions, health and the combat log ---------------------------------------

-- Units stand at world positions (x north, y west, as in maps.lua) on the
-- player's continent; the player's position is the world model's.
local function worldPlayer() return WowProgrammingWorld and WowProgrammingWorld.Player() end

-- A unit with `follow = { north, west }` stays at that offset from the
-- player (party members in the scenarios walk along).
local function position(u)
    if u == player() or u.follow then
        local p = worldPlayer()
        if not p or not p.continent then return nil end
        if u.follow then return p.x + u.follow[1], p.y + u.follow[2] end
        return p.x, p.y
    end
    return u.x, u.y
end

local function distance(a, b)
    local ax, ay = position(a)
    local bx, by = position(b)
    if not ax or not bx then return nil end
    return math.sqrt((ax - bx) ^ 2 + (ay - by) ^ 2)
end
-- In range of a spell: nil without a range or a unit, true when either
-- has no position.
local function inRange(s, u)
    if not s or (s.range or 0) <= 0 or not u then return nil end
    if u == player() then return true end
    local d = distance(player(), u)
    return d == nil or d <= s.range
end
State.Distance = function(unit) local u = resolveUnit(unit) return u and distance(player(), u) end

-- The tokens that name a unit now (for UNIT_* events).
local TOKENS = { "player", "target", "focus", "mouseover", "pet",
                 "party1", "party2", "party3", "party4" }
local extraTokens = {} -- token -> unit, added by the nameplates
local function tokensOf(u)
    local out = {}
    for _, t in ipairs(TOKENS) do
        if units[t] == u then out[#out + 1] = t end
    end
    for t, x in pairs(extraTokens) do
        if x == u then out[#out + 1] = t end
    end
    return out
end
local function fireUnit(event, u, ...)
    for _, t in ipairs(tokensOf(u)) do fire(event, t, ...) end
end
State.TokensOf = tokensOf
-- Extra tokens (nameplateN) name units like the others: every unit
-- function answers for them.
State.SetExtraToken = function(token, u)
    extraTokens[token] = u
    units[token] = u
end

local function hostile(u) return u ~= nil and u.canAttack == true end

-- The combat log. Each entry fires COMBAT_LOG_EVENT_UNFILTERED, and
-- COMBAT_LOG_EVENT when it passes the filters Blizzard's combat log set.
-- The values come from C_CombatLogSecure.GetCurrentEventInfo; in this
-- client that namespace is for Blizzard code only (Environment
-- "SecureOnly"), and C_CombatLog has no GetCurrentEventInfo.
local combatLog = {}
local currentEvent
local MAX_LOG = 500
local NONE = 0x80000000
-- Unit flags (Enum.CombatLogObject): the player (mine, friendly, player),
-- party members, hostile NPCs, friendly NPCs; None without a unit.
local function logFlags(u)
    if not u then return NONE end
    if u == player() then return 0x511 end
    if u.inParty then return 0x512 end
    if u.canAttack then return 0xa48 end
    return 0xa18
end

-- Filters from C_CombatLogSecure.AddEventFilter: { events = set or nil,
-- src = flags or GUID or nil, dst = ... }.
local logFilters = {}
local filteredEnabled = true
-- Flag groups: within a group a filter bit must match one of the unit's
-- bits; every group the filter names must match (inferred).
local FLAG_GROUPS = { 0x0000000F, 0x000000F0, 0x00000300, 0x0000FC00, 0x7FFF0000, NONE }
local function flagsMatch(flags, filter)
    if filter == nil then return true end
    if type(filter) == "string" then return false end
    local band = bit.band
    local any = false
    for _, g in ipairs(FLAG_GROUPS) do
        local want = band(filter, g)
        if want ~= 0 then
            any = true
            if band(flags or 0, want) == 0 then return false end
        end
    end
    return any
end
local function sideMatches(guid, flags, filter)
    if type(filter) == "string" then return guid == filter end
    return flagsMatch(flags, filter)
end
local function entryShown(e)
    if not filteredEnabled then return true end
    for _, f in ipairs(logFilters) do
        if (f.events == nil or f.events[e[2]])
                and sideMatches(e[4], e[6], f.src) and sideMatches(e[8], e[10], f.dst) then
            return true
        end
    end
    return false
end

local function logEvent(sub, src, dst, ...)
    local time = (GetServerTime and GetServerTime() or 0) + now() % 1
    local values = { n = 11 + select("#", ...), time, sub, false,
        src and src.guid or "", src and src.name or nil, logFlags(src), 0,
        dst and dst.guid or "", dst and dst.name or nil, logFlags(dst), 0, ... }
    combatLog[#combatLog + 1] = values
    if #combatLog > MAX_LOG then table.remove(combatLog, 1) end
    queueCall(function()
        currentEvent = values
        fireNow("COMBAT_LOG_EVENT_UNFILTERED")
        if entryShown(values) then fireNow("COMBAT_LOG_EVENT") end
    end)
end
State.CombatLog = combatLog

-- Sets a client function (defined under "Client functions").
local set

-- The combat log API. The entry cursor walks back from the newest entry
-- (refiltering when the filters change).
local function installCombatLog()
    local cursor -- index into combatLog
    local messageLimit, retention = 300, 300
    local function seek(from, ignoreFilter)
        local i = from
        while i >= 1 do
            if ignoreFilter or entryShown(combatLog[i]) then
                cursor = i
                return true
            end
            i = i - 1
        end
        cursor = nil
        -- nil, not false: Blizzard's refilter loop compares with nil.
        return nil
    end
    set("C_CombatLog.ApplyFilterSettings", function(settings)
        fireNow("COMBAT_LOG_APPLY_FILTER_SETTINGS", settings)
    end)
    set("C_CombatLog.RefilterEntries", function() fireNow("COMBAT_LOG_REFILTER_ENTRIES") end)
    set("C_CombatLog.ClearEntries", function()
        for i = #combatLog, 1, -1 do combatLog[i] = nil end
        cursor = nil
        fireNow("COMBAT_LOG_ENTRIES_CLEARED")
    end)
    set("C_CombatLog.DoesObjectMatchFilter", function(flags, mask) return flagsMatch(flags, mask) end)
    set("C_CombatLog.GetMessageLimit", function() return messageLimit end)
    set("C_CombatLog.SetMessageLimit", function(n)
        messageLimit = n
        fireNow("COMBAT_LOG_MESSAGE_LIMIT_CHANGED", n)
    end)
    set("C_CombatLog.GetEntryRetentionTime", function() return retention end)
    set("C_CombatLog.SetEntryRetentionTime", function(n) retention = n end)
    set("C_CombatLog.AreFilteredEventsEnabled", function() return filteredEnabled end)
    set("C_CombatLog.SetFilteredEventsEnabled", function(on) filteredEnabled = on and true or false end)
    set("C_CombatLog.IsCombatLogRestricted", function() return false end)

    -- School masks to their names (Enum.Damageclass bits: physical 1,
    -- holy 2, fire 4, nature 8, frost 16, shadow 32, arcane 64).
    local SCHOOL_NAMES = {
        [1] = "PHYSICAL", [2] = "HOLY", [4] = "FIRE", [8] = "NATURE", [16] = "FROST",
        [32] = "SHADOW", [64] = "ARCANE", [3] = "HOLYSTRIKE", [5] = "FLAMESTRIKE",
        [6] = "HOLYFIRE", [9] = "STORMSTRIKE", [10] = "HOLYSTORM", [12] = "FIRESTORM",
        [17] = "FROSTSTRIKE", [18] = "HOLYFROST", [20] = "FROSTFIRE", [24] = "FROSTSTORM",
        [28] = "ELEMENTAL", [33] = "SHADOWSTRIKE", [34] = "SHADOWHOLY", [36] = "SHADOWFLAME",
        [40] = "SHADOWSTORM", [48] = "SHADOWFROST", [65] = "SPELLSTRIKE", [68] = "SPELLFIRE",
        [72] = "SPELLSTORM", [80] = "SPELLFROST", [96] = "SPELLSHADOW", [124] = "CHROMATIC",
        [126] = "MAGIC", [127] = "CHAOS",
    }
    set("C_Spell.GetSchoolString", function(mask)
        local key = SCHOOL_NAMES[mask]
        return key and _G["STRING_SCHOOL_" .. key] or STRING_SCHOOL_UNKNOWN
    end)

    set("C_CombatLogSecure.AddEventFilter", function(events, src, dst)
        local set_
        if type(events) == "string" and events ~= "" then
            set_ = {}
            for name in events:gmatch("[^,]+") do set_[name] = true end
        end
        logFilters[#logFilters + 1] = { events = set_, src = src, dst = dst }
    end)
    set("C_CombatLogSecure.ClearEventFilters", function() logFilters = {} end)
    set("C_CombatLogSecure.GetCurrentEventInfo", function()
        if currentEvent then return unpack(currentEvent, 1, currentEvent.n) end
    end)
    set("C_CombatLogSecure.GetCurrentEntryInfo", function()
        local e = cursor and combatLog[cursor]
        if e then return unpack(e, 1, e.n) end
    end)
    set("C_CombatLogSecure.GetEntryCount", function(ignoreFilter)
        if ignoreFilter then return #combatLog end
        local n = 0
        for _, e in ipairs(combatLog) do
            if entryShown(e) then n = n + 1 end
        end
        return n
    end)
    set("C_CombatLogSecure.SeekToNewestEntry", function(ignoreFilter) return seek(#combatLog, ignoreFilter) end)
    set("C_CombatLogSecure.SeekToPreviousEntry", function(ignoreFilter)
        if not cursor then return nil end
        return seek(cursor - 1, ignoreFilter)
    end)
    set("C_CombatLogSecure.ShouldShowCurrentEntry", function()
        local e = cursor and combatLog[cursor]
        return e ~= nil and entryShown(e)
    end)
    set("C_CombatLogSecure.CreateCombatLogMessage", function(text, r, g, b, order)
        fireNow("COMBAT_LOG_MESSAGE", text, r, g, b, order)
    end)
end


-- Defined with the death cycle below casting.
local onPlayerDeath

local function die(u, killer)
    u.dead, u.health = true, 0
    for _, a in ipairs(auras[u] or {}) do
        removeAura(u, a.auraInstanceID)
    end
    if killer == player() or (killer and killer.inParty) then logEvent("PARTY_KILL", killer, u) end
    logEvent("UNIT_DIED", nil, u, "", false)
    if u == player() then onPlayerDeath() end
    if u.respawn then
        C_Timer.After(u.respawn, function()
            -- Back where it started (a unit that fought moved).
            if u.homeX then u.x, u.y = u.homeX, u.homeY end
            u.dead, u.health = false, u.healthMax or 100
            fireUnit("UNIT_HEALTH", u)
        end)
    end
end

local function setHealth(u, value)
    local max = u.healthMax or 100
    u.health = math.max(0, math.min(max, value))
    fireUnit("UNIT_HEALTH", u)
end

local SCHOOL = { physical = 1, holy = 2, fire = 4, nature = 8, frost = 16, shadow = 32, arcane = 64 }
local function school(s) return SCHOOL[s.school or ""] or 1 end

-- Damage from a spell; overkill is -1 when the unit survives (as in the
-- game's combat log).
-- Floating combat text (Blizzard_CombatText) shows what happens to one
-- unit, the player unless C_CombatText.SetActiveUnit names another:
-- COMBAT_TEXT_UPDATE(type), and C_CombatText.GetCurrentEventInfo returns
-- (data, arg3, arg4): the amount for damage; the healer's name and the
-- amount for heals (CombatText.lua).
local combatTextUnit = "player"
local combatTextInfo = {}
local function combatText(u, kind, data, arg3, arg4)
    if resolveUnit(combatTextUnit) ~= u then return end
    queueCall(function()
        combatTextInfo = { data, arg3, arg4 }
        fireNow("COMBAT_TEXT_UPDATE", kind)
    end)
end

-- Damage from a spell, or a melee hit without one (SWING_DAMAGE).
local function dealDamage(src, dst, s, amount, periodic)
    if dst.dead or dst.ghost then return end
    local before = dst.health or dst.healthMax or 100
    local mask = s and school(s) or 1
    setHealth(dst, before - amount)
    -- UNIT_COMBAT drives the hit numbers on the player and pet frames.
    fireUnit("UNIT_COMBAT", dst, "WOUND", "", amount, mask)
    combatText(dst, s and s.id ~= 0 and "SPELL_DAMAGE" or "DAMAGE", amount)
    local overkill = amount > before and amount - before or -1
    if s then
        logEvent(periodic and "SPELL_PERIODIC_DAMAGE" or "SPELL_DAMAGE", src, dst,
            s.id, s.name, mask, amount, overkill, mask, nil, nil, nil, false, false, false, false)
    else
        logEvent("SWING_DAMAGE", src, dst, amount, overkill, mask, nil, nil, nil, false, false, false, false)
    end
    if src == player() or dst == player() then enterCombat() end
    if src == player() and dst.attack and State.Engage then State.Engage(dst) end
    if dst.health <= 0 then die(dst, src) end
end

local function heal(src, dst, s, amount, periodic)
    if dst.dead or dst.ghost then return end
    local before = dst.health or dst.healthMax or 100
    local max = dst.healthMax or 100
    local actual = math.min(amount, max - before)
    setHealth(dst, before + actual)
    fireUnit("UNIT_COMBAT", dst, "HEAL", "", amount, school(s))
    combatText(dst, periodic and "PERIODIC_HEAL" or "HEAL", src and src.name or "", amount)
    logEvent(periodic and "SPELL_PERIODIC_HEAL" or "SPELL_HEAL", src, dst,
        s.id, s.name, school(s), amount, amount - actual, nil, false)
end
State.Damage = function(unit, amount)
    local u = resolveUnit(unit)
    if u then dealDamage(nil, u, { id = 0, name = "", school = "physical" }, amount) end
end

-- Casting ---------------------------------------------------------------------

local function castGUID()
    local guid = string.format("Cast-3-0-0-0-%d-%010d", 0, nextCastID)
    nextCastID = nextCastID + 1
    return guid
end

local function isHarmful(s) return s.harmful or (s.aura and s.aura.harmful) or false end
-- A helpful spell that acts on another unit (a heal, a heal over time).
local function isTargetedHelp(s)
    return not isHarmful(s) and (s.heal ~= nil or (s.aura and s.aura.self == false))
end

local function spellTarget(s, unit)
    if unit then return resolveUnit(unit) end
    if isHarmful(s) then return units.target end
    -- Helpful spells go to a friendly target, else to the player.
    if isTargetedHelp(s) and units.target and not hostile(units.target) then return units.target end
    return player()
end

-- The spell's effects when it lands: costs are paid at the start (inferred),
-- auras applied here.
local function land(s, target)
    if isHarmful(s) and target and target.attack then State.Engage(target) end
    if s.damage and target then dealDamage(player(), target, s, s.damage) end
    if s.heal and target then heal(player(), target, s, s.heal) end
    if s.aura then
        local u = s.aura.harmful and target or (s.aura.self ~= false and player()) or target
        if u then applyAura(u, s, s.aura) end
    end
    if s.restore then
        local u = player()
        setPower(u, (u.power or 0) + s.restore)
    end
end

local function startCooldowns(s)
    local t = now()
    if s.gcd ~= 0 then
        gcdStart, gcdDuration = t, s.gcd or GCD
    end
    if (s.cooldown or 0) > 0 then
        cooldowns[s.id] = { start = t, duration = s.cooldown }
    end
    fire("SPELL_UPDATE_COOLDOWN", s.id, s.id)
    fire("ACTIONBAR_UPDATE_COOLDOWN")
end

local function finishCast(ok)
    local c = cast
    if not c then return end
    cast = nil
    local unit = "player"
    if c.channel then
        fire("UNIT_SPELLCAST_CHANNEL_STOP", unit, c.guid, c.spell.id)
    elseif ok then
        fire("UNIT_SPELLCAST_STOP", unit, c.guid, c.spell.id)
        fire("UNIT_SPELLCAST_SUCCEEDED", unit, c.guid, c.spell.id)
        land(c.spell, c.target)
    else
        fire("UNIT_SPELLCAST_INTERRUPTED", unit, c.guid, c.spell.id)
        fire("UNIT_SPELLCAST_STOP", unit, c.guid, c.spell.id)
    end
    fire("CURRENT_SPELL_CAST_CHANGED", not ok)
    fire("ACTIONBAR_UPDATE_STATE")
end

local function failCast(s, reason)
    fire("UNIT_SPELLCAST_FAILED", "player", castGUID(), s.id)
    if reason then fire("UI_ERROR_MESSAGE", 0, reason) end
    return false
end

-- Start casting a spell. Every spell succeeds, unless it is on cooldown,
-- the player lacks power, or another cast is in progress.
local function castSpell(s, unit)
    if not s or s.passive then return false end
    local me = player()
    if me.dead or me.ghost then return failCast(s, SPELL_FAILED_CASTER_DEAD) end
    -- Spells with a cast time or a channel need the player to stand still.
    if ((s.castTime or 0) > 0 or (s.channel or 0) > 0) and IsPlayerMoving and IsPlayerMoving() then
        return failCast(s, SPELL_FAILED_MOVING)
    end
    if cast then return failCast(s, SPELL_FAILED_SPELL_IN_PROGRESS) end
    local cd = spellCooldown(s)
    if cd.isActive then
        return failCast(s, cd.isOnGCD and SPELL_FAILED_NOT_READY or SPELL_FAILED_NOT_READY)
    end
    local ok, noPower = usable(s)
    if not ok then return failCast(s, noPower and ERR_OUT_OF_MANA or nil) end
    local target = spellTarget(s, unit)
    if isHarmful(s) then
        if not target then return failCast(s, ERR_GENERIC_NO_TARGET or SPELL_FAILED_BAD_TARGETS) end
        if not hostile(target) then return failCast(s, SPELL_FAILED_BAD_TARGETS) end
    end
    if target and target.dead then return failCast(s, SPELL_FAILED_TARGETS_DEAD) end
    -- Range from positions (units without a position are in range).
    if target and target ~= player() and (s.range or 0) > 0 then
        local d = distance(player(), target)
        if d and d > s.range then return failCast(s, SPELL_FAILED_OUT_OF_RANGE or ERR_OUT_OF_RANGE) end
    end
    local u = player()
    local guid = castGUID()
    if isHarmful(s) then enterCombat() end
    fire("UNIT_SPELLCAST_SENT", "player", target and target.name or "", guid, s.id)
    if s.cost then setPower(u, (u.power or 0) - s.cost) end
    startCooldowns(s)
    local t = now()
    if (s.channel or 0) > 0 then
        cast = { spell = s, guid = guid, start = t, finish = t + s.channel, channel = true, target = target,
                 nextTick = t + 1 }
        fire("UNIT_SPELLCAST_CHANNEL_START", "player", guid, s.id)
        land(s, target)
    elseif (s.castTime or 0) > 0 then
        cast = { spell = s, guid = guid, start = t, finish = t + s.castTime, target = target }
        fire("UNIT_SPELLCAST_START", "player", guid, s.id)
    else
        fire("UNIT_SPELLCAST_SUCCEEDED", "player", guid, s.id)
        land(s, target)
    end
    fire("CURRENT_SPELL_CAST_CHANGED", false)
    fire("ACTIONBAR_UPDATE_STATE")
    return true
end
State.CastSpell = function(id, unit) return castSpell(findSpell(id), unit) end

-- Time ------------------------------------------------------------------------

local regenTimer = 0
-- Hostile units that fight back -------------------------------------------------

-- A unit with `attack = { damage, interval }` engages when the player
-- hurts it: it runs to melee range and hits the player every `interval`
-- seconds (SWING_DAMAGE). Pulled more than 60 yards from where it
-- started, it goes back at full health. These are simple rules, not the
-- game's (no threat, no pathing).
local MELEE_RANGE, LEASH, MOB_SPEED = 5, 60, 7
local engaged = {}
function State.Engage(u)
    local me = player()
    if engaged[u] or u.dead or not u.attack or me.dead or me.ghost then return end
    engaged[u] = true
    u.homeX, u.homeY = u.homeX or u.x, u.homeY or u.y
    u.nextSwing = now() + 0.5
end
function State.Engaged(unit)
    local u = hidden[unit] or resolveUnit(unit)
    return u ~= nil and engaged[u] == true
end

local function fightBack(t, elapsed)
    local me = player()
    for u in pairs(engaged) do
        local px, py = position(me)
        if u.dead or me.dead or me.ghost or not px or not u.x then
            engaged[u] = nil
        elseif math.sqrt((u.x - u.homeX) ^ 2 + (u.y - u.homeY) ^ 2) > LEASH then
            engaged[u] = nil
            u.x, u.y = u.homeX, u.homeY
            setHealth(u, u.healthMax or 100)
        else
            local dx, dy = px - u.x, py - u.y
            local d = math.sqrt(dx * dx + dy * dy)
            if d > MELEE_RANGE then
                local step = math.min(MOB_SPEED * elapsed, d - MELEE_RANGE + 0.5)
                u.x, u.y = u.x + dx / d * step, u.y + dy / d * step
            elseif t >= u.nextSwing then
                u.nextSwing = t + (u.attack.interval or 2)
                dealDamage(u, me, nil, u.attack.damage or 1)
            end
        end
    end
end

-- The player's death ------------------------------------------------------------

-- At 0 health the player dies (PLAYER_DEAD; Blizzard shows the release
-- dialog). RepopMe releases the spirit: a ghost (PLAYER_ALIVE) at a
-- graveyard 60 yards south of the corpse (invented). Within 39 yards of
-- the corpse the client sends CORPSE_IN_RANGE (inferred distance) and
-- RetrieveCorpse brings the player back at half health (PLAYER_UNGHOST).
local RELEASE_TIME, CORPSE_RANGE, GRAVEYARD_SOUTH = 360, 39, 60
local corpse, deathTime, corpseInRange
local function setFrozen(on)
    if WowProgrammingWorld and WowProgrammingWorld.SetFrozen then WowProgrammingWorld.SetFrozen(on) end
end
onPlayerDeath = function()
    if cast then finishCast(false) end
    local p = worldPlayer()
    corpse = p and p.continent and { continent = p.continent, x = p.x, y = p.y } or nil
    deathTime = now()
    for u in pairs(engaged) do engaged[u] = nil end
    setFrozen(true)
    if inCombat then leaveCombat() end
    fire("PLAYER_DEAD")
end

local function corpseDistance()
    local p = worldPlayer()
    if not corpse or not p or p.continent ~= corpse.continent then return nil end
    return math.sqrt((p.x - corpse.x) ^ 2 + (p.y - corpse.y) ^ 2)
end

local function repop()
    local me = player()
    if not me.dead then return end
    me.dead, me.ghost, me.health = false, true, 1
    setFrozen(false)
    if corpse then
        WowProgrammingWorld.SetPlayerWorldPosition(corpse.continent, corpse.x - GRAVEYARD_SOUTH, corpse.y)
    end
    corpseInRange = false
    fireUnit("UNIT_HEALTH", me)
    fire("PLAYER_ALIVE")
end

local function retrieveCorpse()
    local me = player()
    local d = corpseDistance()
    if not me.ghost or (d and d > CORPSE_RANGE) then return end
    me.ghost = false
    corpse, corpseInRange = nil, nil
    setHealth(me, math.floor((me.healthMax or 100) / 2))
    fire("PLAYER_UNGHOST")
end

local function watchCorpse()
    if not player().ghost then return end
    local d = corpseDistance()
    local near = d ~= nil and d <= CORPSE_RANGE
    if near ~= corpseInRange then
        corpseInRange = near
        fire(near and "CORPSE_IN_RANGE" or "CORPSE_OUT_OF_RANGE")
    end
end

local function corpseMapPosition(mapID)
    if not corpse then return nil end
    local continent = C_Map.GetWorldPosFromMapPos(mapID, CreateVector2D(0.5, 0.5))
    if continent ~= corpse.continent then return nil end
    local _, pos = C_Map.GetMapPosFromWorldPos(corpse.continent, CreateVector2D(corpse.x, corpse.y), mapID)
    return pos
end

-- Nameplates -------------------------------------------------------------------

-- The client gives a nameplate to each unit near the player that the
-- settings show (nameplateShowEnemies, nameplateShowFriends for players,
-- nameplateShowFriendlyNPCs), within nameplateMaxDistance yards. Each
-- plate is a frame the client makes ("NamePlateN", NAME_PLATE_CREATED
-- once), then a unit token nameplateN (NAME_PLATE_UNIT_ADDED, and
-- NAME_PLATE_UNIT_REMOVED when it goes). The client moves the frame over
-- the unit's head every frame; a unit behind the camera keeps its plate,
-- hidden (C_NamePlateManager.IsNamePlateUnitBehindCamera).
local HEAD_HEIGHT = 2.6 -- yards; one height for every unit (invented)
local plates, plateUnit, plateOf = {}, {}, {}
local plateWidth, plateHeight = 110, 45

local function cvarOn(name, default)
    local v = GetCVar and GetCVar(name)
    if v == nil then return default end
    return v == "1"
end

local function wantsPlate(u, me)
    if u == me or u.dead or u.ghost then return false end
    local d = distance(me, u)
    if not d or d > (tonumber(GetCVar and GetCVar("nameplateMaxDistance")) or 41) then return false end
    if hostile(u) then return cvarOn("nameplateShowEnemies", true) end
    if u.isPlayer then return cvarOn("nameplateShowFriends", false) end
    return cvarOn("nameplateShowFriendlyNPCs", false)
end

-- Native NamePlate frame methods: the hit-test region is a list of
-- anchor points (Blizzard_NamePlateUnitFrame.lua sets it from the name
-- and the health bar).
local function addPlateMethods(f)
    f.hitTestPoints = {}
    function f:SetHitTestPoints(points) self.hitTestPoints = points or {} end
    function f:ClearAllHitTestPoints() self.hitTestPoints = {} end
    function f:GetHitTestPoints() return self.hitTestPoints end
    function f:SetAllHitTestPoints(relativeTo)
        self.hitTestPoints = {
            { point = "TOPLEFT", relativeTo = relativeTo or self, relativePoint = "TOPLEFT" },
            { point = "BOTTOMRIGHT", relativeTo = relativeTo or self, relativePoint = "BOTTOMRIGHT" },
        }
    end
    function f:CanChangeHitTestPoints() return true end
    function f:SetStackingBoundsFrame() end
end

local function addPlate(u)
    local i = 1
    while plateUnit[i] do i = i + 1 end
    local f = plates[i]
    if not f then
        f = CreateFrame("Frame", "NamePlate" .. i, WorldFrame)
        f:SetSize(plateWidth, plateHeight)
        addPlateMethods(f)
        plates[i] = f
        fire("NAME_PLATE_CREATED", f)
    end
    local token = "nameplate" .. i
    plateUnit[i], plateOf[u] = u, i
    f.namePlateUnitToken = token
    State.SetExtraToken(token, u)
    fire("NAME_PLATE_UNIT_ADDED", token)
end

local function removePlate(i)
    local u, f = plateUnit[i], plates[i]
    local token = "nameplate" .. i
    plateUnit[i], plateOf[u] = nil, nil
    f:Hide()
    queueCall(function()
        fireNow("NAME_PLATE_UNIT_REMOVED", token)
        -- Unless the index went to another unit meanwhile.
        if not plateUnit[i] then State.SetExtraToken(token, nil) end
    end)
end

local function headOnScreen(u)
    local x, y = position(u)
    if not x then return nil end
    return WowProgrammingWorld.ProjectToScreen(x, y, HEAD_HEIGHT)
end

local function updatePlates()
    local me = player()
    if not WorldFrame or not WowProgrammingWorld then return end
    local want, list = {}, {}
    for _, group in ipairs({ hidden, units }) do
        for _, u in pairs(group) do
            if not want[u] and wantsPlate(u, me) then
                want[u] = true
                list[#list + 1] = u
            end
        end
    end
    for u, i in pairs(plateOf) do
        if not want[u] then removePlate(i) end
    end
    -- Nearest first, so the numbers do not depend on table order.
    table.sort(list, function(a, b) return distance(me, a) < distance(me, b) end)
    for _, u in ipairs(list) do
        if not plateOf[u] then addPlate(u) end
    end
    for i, u in pairs(plateUnit) do
        local f = plates[i]
        local sx, sy = headOnScreen(u)
        if sx then
            f:ClearAllPoints()
            f:SetPoint("BOTTOM", WorldFrame, "BOTTOMLEFT", sx, sy)
            f:Show()
        else
            f:Hide()
        end
    end
end

-- A point of a frame in screen units.
local function framePoint(frame, point)
    local l, b, w, h = frame:GetRect()
    if not l then return nil end
    local s = frame:GetEffectiveScale()
    local x = point:find("LEFT") and l or point:find("RIGHT") and l + w or l + w / 2
    local y = point:find("TOP") and b + h or point:find("BOTTOM") and b or b + h / 2
    return x * s, y * s
end

local function plateContains(f, x, y)
    if not f:IsVisible() then return false end
    local pts = f.hitTestPoints
    local l, t, r, b
    if pts and #pts >= 2 then
        for _, p in ipairs(pts) do
            local px, py = framePoint(p.relativeTo or f, p.relativePoint or p.point)
            if not px then return false end
            px, py = px + (p.offsetX or 0), py + (p.offsetY or 0)
            if p.point:find("LEFT") then l = px elseif p.point:find("RIGHT") then r = px end
            if p.point:find("TOP") then t = py elseif p.point:find("BOTTOM") then b = py end
        end
    else
        l, t = framePoint(f, "TOPLEFT")
        r, b = framePoint(f, "BOTTOMRIGHT")
    end
    return l and r and t and b and x >= l and x <= r and y >= b and y <= t
end

-- A click in the 3D world (the engine calls this when no frame takes
-- it): a nameplate under the mouse, else the unit whose body (from the
-- ground to its head) is nearest the mouse within 30 screen units, gets
-- targeted. A left click on nothing clears the target unless sticky
-- targeting is on (deselectOnClick "0").
local PICK_RADIUS = 30
function WowProgramming_WorldClick(button, x, y)
    for i, u in pairs(plateUnit) do
        if plateContains(plates[i], x, y) then
            setTarget(u)
            return
        end
    end
    local best, bestD
    for _, group in ipairs({ hidden, units }) do
        for _, u in pairs(group) do
            local ux, uy = position(u)
            if u ~= player() and ux then
                for _, z in ipairs({ 0.3, 1.2, 2.1 }) do
                    local sx, sy = WowProgrammingWorld.ProjectToScreen(ux, uy, z)
                    if sx then
                        local d = math.sqrt((sx - x) ^ 2 + (sy - y) ^ 2)
                        if d <= PICK_RADIUS and (not bestD or d < bestD) then best, bestD = u, d end
                    end
                end
            end
        end
    end
    if best then
        setTarget(best)
    elseif button == "LeftButton" and cvarOn("deselectOnClick", true) then
        setTarget(nil)
    end
end
-- The units the 3D view draws: living units with a position, facing the
-- player (followers face the player's way). Each entry: x, y (world yards), yaw (radians), raceID and sex
-- (0 male, 1 female) for characters, else model (a creature model file
-- ID, optional) and index (a stable number to pick a stand-in model).
State.WorldUnits = function()
    local me = player()
    local px, py = position(me)
    local out, seen = {}, {}
    for _, group in ipairs({ hidden, units }) do
        for _, u in pairs(group) do
            local x, y = position(u)
            if not seen[u] and u ~= me and x and not u.dead and not u.ghost then
                seen[u] = true
                -- Followers face where the player faces; others face it.
                local yaw = px and math.atan2(py - y, px - x) or 0
                local wp = worldPlayer()
                if u.follow and wp then yaw = wp.facing end
                out[#out + 1] = { x = x, y = y, yaw = yaw, raceID = u.raceID,
                    sex = u.sex == 3 and 1 or 0, model = u.model, key = u.guid or u.name or "" }
            end
        end
    end
    table.sort(out, function(a, b) return a.key < b.key end)
    for i, e in ipairs(out) do e.index = i end
    return out
end
State.HeadOnScreen = function(unit)
    local u = hidden[unit] or resolveUnit(unit)
    if u then return headOnScreen(u) end
end
-- Where a unit appears on the screen at a height (yards, default 1.2:
-- the middle of its body), for specs that click units in the world.
State.BodyOnScreen = function(unit, height)
    local u = hidden[unit] or resolveUnit(unit)
    if not u then return nil end
    local x, y = position(u)
    if x then return WowProgrammingWorld.ProjectToScreen(x, y, height or 1.2) end
end

-- Client functions for death and nameplates (kept out of install(),
-- which would pass Lua 5.1's limit of 60 upvalues).
local function installWorld()
    set("C_NamePlate.GetNamePlateForUnit", function(unit)
        -- A nameplate token keeps its frame until NAME_PLATE_UNIT_REMOVED
        -- has been handled.
        local n = type(unit) == "string" and tonumber(unit:lower():match("^nameplate(%d+)$"))
        if n then return extraTokens["nameplate" .. n] and plates[n] or nil end
        local u = resolveUnit(unit)
        local i = u and plateOf[u]
        return i and plates[i] or nil
    end)
    set("C_NamePlate.GetNamePlates", function()
        local out = {}
        for i in pairs(plateUnit) do out[#out + 1] = plates[i] end
        return out
    end)
    set("C_NamePlate.SetNamePlateSize", function(w, h)
        plateWidth, plateHeight = w, h
        for _, f in pairs(plates) do f:SetSize(w, h) end
    end)
    set("C_NamePlateManager.IsNamePlateUnitBehindCamera", function(unit)
        local u = resolveUnit(unit)
        return u ~= nil and headOnScreen(u) == nil
    end)
    set("C_NamePlateManager.SetNamePlateSimplified", function() end)
    -- Whether the client shows a unit's name (over its head and on its
    -- nameplate), from the unit name settings (inferred mapping).
    set("UnitShouldDisplayName", function(unit)
        local u = resolveUnit(unit)
        if not u then return false end
        if u == player() then return cvarOn("UnitNameOwn", false) end
        if u.isPlayer then
            if hostile(u) then return cvarOn("UnitNameEnemyPlayerName", true) end
            return cvarOn("UnitNameFriendlyPlayerName", true)
        end
        if hostile(u) then return cvarOn("UnitNameHostleNPC", true) end
        return cvarOn("UnitNameNPC", false) or cvarOn("UnitNameFriendlySpecialNPCName", true)
    end)
    set("RepopMe", repop)
    set("RetrieveCorpse", retrieveCorpse)
    set("GetReleaseTimeRemaining", function()
        if not player().dead or not deathTime then return 0 end
        return math.max(0, RELEASE_TIME - (now() - deathTime))
    end)
    set("GetCorpseRecoveryDelay", function() return 0 end)
    set("CannotBeResurrected", function() return false end)
    set("C_DeathInfo.GetCorpseMapPosition", function(mapID)
        local me = player()
        if me.dead or me.ghost then return corpseMapPosition(mapID) end
    end)
end

local gcdWasActive = false
-- Action slots with range checks on (C_ActionBar.EnableActionRangeCheck):
-- slot -> { count, inRange, checks }.
local rangeWatch = {}
local function tick(elapsed)
    local t = now()
    if inCombat and t - lastHostile > COMBAT_TIMEOUT then leaveCombat() end
    -- Moving interrupts a cast or a channel.
    if cast and IsPlayerMoving and IsPlayerMoving() then finishCast(false) end
    -- A channel deals its tick damage once a second.
    if cast and cast.channel and cast.spell.tickDamage and cast.target then
        while cast.nextTick <= t and cast.nextTick <= cast.finish do
            dealDamage(player(), cast.target, cast.spell, cast.spell.tickDamage, true)
            cast.nextTick = cast.nextTick + 1
        end
    end
    if cast and t >= cast.finish then finishCast(true) end
    fightBack(t, elapsed)
    watchCorpse()
    updatePlates()
    -- Auras that heal or hurt every few seconds.
    for u, list in pairs(auras) do
        for _, a in ipairs(list) do
            local p = a.periodic
            if p and t >= p.next then
                p.next = p.next + p.interval
                if p.heal then heal(p.source, u, p.spell, p.heal, true) end
                if p.damage then dealDamage(p.source, u, p.spell, p.damage, true) end
            end
        end
    end
    -- Range of the action slots the bars watch: an event when it changes.
    for slot, w in pairs(rangeWatch) do
        local s = actionSpell(slot)
        local u = s and (s.range or 0) > 0 and spellTarget(s)
        local checks = u ~= nil and u ~= false and u ~= player()
        local ok = checks and inRange(s, u) == true
        if ok ~= w.inRange or checks ~= w.checks then
            w.inRange, w.checks = ok, checks
            fire("ACTION_RANGE_CHECK_UPDATE", slot, ok, checks)
        end
    end
    -- Cooldowns ending: the client sends SPELL_UPDATE_COOLDOWN (inferred).
    local gcdActive = gcdStart + gcdDuration > t
    if gcdWasActive and not gcdActive then
        fire("SPELL_UPDATE_COOLDOWN")
        fire("ACTIONBAR_UPDATE_COOLDOWN")
    end
    gcdWasActive = gcdActive
    for id, cd in pairs(cooldowns) do
        if cd.start + cd.duration <= t then
            cooldowns[id] = nil
            fire("SPELL_UPDATE_COOLDOWN", id, id)
            fire("ACTIONBAR_UPDATE_COOLDOWN")
            fire("SPELL_UPDATE_USABLE")
        end
    end
    for u, list in pairs(auras) do
        for i = #list, 1, -1 do
            local a = list[i]
            if a.expirationTime > 0 and a.expirationTime <= t then removeAura(u, a.auraInstanceID) end
        end
    end
    -- Power comes back over time: a tenth of the maximum each second
    -- outside a cast (a simple rule, not the game's).
    regenTimer = regenTimer + elapsed
    if regenTimer >= 0.5 then
        local u = player()
        if u and (u.power or 0) < (u.powerMax or 0) and not cast then
            setPower(u, (u.power or 0) + math.ceil((u.powerMax or 0) * 0.05))
            fire("SPELL_UPDATE_USABLE")
            fire("ACTIONBAR_UPDATE_USABLE")
        end
        regenTimer = 0
    end
end

-- Tooltips ----------------------------------------------------------------------

-- C_TooltipInfo data built from the model. Spell and action tooltips
-- follow the client's lines in the Monk capture: the name (SpellName,
-- with an empty grey right text), cost and range, cast time and cooldown
-- (or a SpellPassive line), then the description in gold, wrapped. Aura
-- and unit tooltips have no capture yet; their lines are inferred.
local LINE = { None = 0, UnitName = 2, SpellName = 13, NestedBlock = 19, SpellPassive = 33,
               SpellDescription = 34, UnitLevel = 47 } -- Enum.TooltipDataLineType
local DATA = { Spell = 1, Unit = 2, UnitAura = 7, Macro = 25 } -- Enum.TooltipDataType
local nextDataID = 1

local function rgb(r, g, b) return CreateColor(r, g, b, 1) end
local function white() return rgb(1, 1, 1) end
local function gold() return rgb(1, 0.82353, 0) end
local function grey() return rgb(0.50196, 0.50196, 0.50196) end

local function line(kind, left, leftColor, right, rightColor, wrap)
    return { type = kind, leftText = left, leftColor = leftColor or white(),
             rightText = right, rightColor = right and (rightColor or white()) or nil,
             wrapText = wrap or nil }
end

local function tooltipData(kind, id, lines, extra)
    local d = { type = kind, id = id, lines = lines, dataInstanceID = nextDataID }
    nextDataID = nextDataID + 1
    for k, v in pairs(extra or {}) do d[k] = v end
    return d
end

local function fmt(pattern, fallback, ...)
    local p = type(pattern) == "string" and pattern or fallback
    return string.format(p, ...)
end

local function number(n)
    return BreakUpLargeNumbers and BreakUpLargeNumbers(n) or tostring(n)
end

local function costText(s)
    if not s.cost then return nil end
    local token = powerToken(player() or {})
    return fmt(_G[token .. "_COST"], "%s " .. token, number(s.cost))
end

local function rangeText(s)
    local r = s.range or 0
    if r <= 0 then return nil end
    if r <= 5 then return MELEE_RANGE or "Melee Range" end
    return fmt(SPELL_RANGE, "%s yd range", tostring(r))
end

local function castText(s)
    if (s.channel or 0) > 0 then return SPELL_CAST_CHANNELED or "Channeled" end
    if (s.castTime or 0) > 0 then return fmt(SPELL_CAST_TIME_SEC, "%.2g sec cast", s.castTime) end
    return SPELL_CAST_TIME_INSTANT or "Instant"
end

local function cooldownText(s)
    local cd = s.cooldown or 0
    if cd <= 0 then return "" end
    if cd >= 60 then return fmt(SPELL_RECAST_TIME_MIN, "%.2g min cooldown", cd / 60) end
    return fmt(SPELL_RECAST_TIME_SEC, "%.2g sec cooldown", cd)
end

local function spellTooltip(s)
    if not s then return nil end
    local lines = { line(LINE.SpellName, s.name, white(), "", grey()) }
    local cost, range = costText(s), rangeText(s)
    if cost then
        lines[#lines + 1] = line(LINE.None, cost, nil, range or "")
    else
        lines[#lines + 1] = line(LINE.None, range or "", nil, "")
    end
    if s.passive then
        lines[#lines + 1] = line(LINE.SpellPassive, SPELL_PASSIVE or "Passive")
    else
        lines[#lines + 1] = line(LINE.None, castText(s), nil, cooldownText(s))
    end
    if s.description and s.description ~= "" then
        lines[#lines + 1] = line(LINE.SpellDescription, s.description, gold(), nil, nil, true)
    end
    return tooltipData(DATA.Spell, s.id, lines)
end

-- Time left on an aura, rounded up (inferred), in the client's strings.
local function remainingText(left)
    if left < 60 then
        return fmt(SPELL_TIME_REMAINING_SEC, "%d |4second:seconds; remaining", math.ceil(left))
    elseif left < 3600 then
        return fmt(SPELL_TIME_REMAINING_MIN, "%d |4minute:minutes; remaining", math.ceil(left / 60))
    end
    return fmt(SPELL_TIME_REMAINING_HOURS, "%d |4hour:hours; remaining", math.ceil(left / 3600))
end

-- Inferred: the name with a debuff's dispel type on the right, the
-- description in gold, then the time left.
local function auraTooltip(a)
    if not a then return nil end
    local lines = { line(LINE.SpellName, a.name, white(), a.isHarmful and a.dispelName or nil) }
    if a.description and a.description ~= "" then
        lines[#lines + 1] = line(LINE.SpellDescription, a.description, gold(), nil, nil, true)
    end
    local left = (a.expirationTime or 0) - now()
    if (a.duration or 0) > 0 and left > 0 then
        lines[#lines + 1] = line(LINE.None, remainingText(left), gold())
    end
    return tooltipData(DATA.UnitAura, a.spellId, lines)
end

-- Inferred: the name (Blizzard colors it by reaction or class from
-- unitToken), the level line, and a player's faction.
local function unitTooltip(token)
    local u = unitData(token)
    if not u then return nil end
    local lines = { line(LINE.UnitName, u.name or UNKNOWN or "Unknown") }
    lines[1].unitToken = token
    local level = (u.level or 0) < 0 and "??" or tostring(u.level or 1)
    local text
    if u.isPlayer then
        text = fmt(TOOLTIP_UNIT_LEVEL_RACE, "Level %s %s", level, u.race or "")
            .. " " .. (u.class or "") .. " (" .. (PLAYER or "Player") .. ")"
    else
        text = fmt(TOOLTIP_UNIT_LEVEL_RACE, "Level %s %s", level, u.creatureType or "")
    end
    lines[2] = line(LINE.UnitLevel, text)
    if u.isPlayer and u.faction then
        lines[3] = line(LINE.None, _G["FACTION_" .. u.faction:upper()] or u.faction)
    end
    return tooltipData(DATA.Unit, nil, lines, { guid = u.guid, healthGUID = u.guid })
end

local function findAura(unit, pred)
    local u = unitData(unit)
    for _, a in ipairs(u and auras[u] or {}) do
        if pred(a) then return a end
    end
end

local function auraAt(unit, index, filter)
    local u = unitData(unit)
    return u and filtered(u, filter)[index]
end

local function byInstance(unit, id)
    return auraTooltip(findAura(unit, function(a) return a.auraInstanceID == id end))
end

State.tooltipFunctions = {
    GetSpellByID = function(id) return spellTooltip(findSpell(id)) end,
    GetSpellBookItem = function(slot, bank) return spellTooltip(bookSpell(slot, bank)) end,
    GetAction = function(slot)
        local m = actionMacro(slot)
        if not m then return spellTooltip(actionSpell(slot)) end
        -- As in the Monk capture: a Macro tooltip is a NestedBlock line,
        -- then the shown spell's lines; without a spell, the name.
        local a = actions[slot]
        local d = spellTooltip(macroSpell(m))
        if d then
            table.insert(d.lines, 1, line(LINE.NestedBlock, "", gold()))
        else
            d = tooltipData(DATA.Macro, a.id, { line(LINE.None, m.name) })
        end
        d.type, d.id = DATA.Macro, a.id
        return d
    end,
    GetUnit = function(unit) return unitTooltip(unit) end,
    GetUnitAura = function(unit, index, filter) return auraTooltip(auraAt(unit, index, filter)) end,
    GetUnitBuff = function(unit, index, filter)
        return auraTooltip(auraAt(unit, index, "HELPFUL" .. (filter and ("|" .. filter) or "")))
    end,
    GetUnitDebuff = function(unit, index, filter)
        return auraTooltip(auraAt(unit, index, "HARMFUL" .. (filter and ("|" .. filter) or "")))
    end,
    GetUnitAuraByAuraInstanceID = byInstance,
    GetUnitBuffByAuraInstanceID = byInstance,
    GetUnitDebuffByAuraInstanceID = byInstance,
}

-- Client functions --------------------------------------------------------------

function set(path, fn)
    local container, key = _G, path
    local ns, name = path:match("^([^.]+)%.(.+)$")
    if ns then
        container = _G[ns]
        if type(container) ~= "table" then
            container = {}
            _G[ns] = container
        end
        key = name
    end
    container[key] = fn
end

local function install()
    -- Spells.
    set("C_Spell.DoesSpellExist", function(id) return findSpell(id) ~= nil end)
    set("C_Spell.GetSpellInfo", function(id)
        local s = findSpell(id)
        if not s then return nil end
        return { name = s.name, iconID = s.icon, originalIconID = s.icon,
            castTime = math.floor((s.castTime or 0) * 1000), minRange = 0,
            maxRange = s.range or 0, spellID = s.id }
    end)
    set("C_Spell.GetSpellName", function(id) local s = findSpell(id) return s and s.name end)
    set("C_Spell.GetSpellTexture", function(id)
        local s = findSpell(id)
        if s then return s.icon, s.icon end
    end)
    set("C_Spell.GetSpellSubtext", function(id) local s = findSpell(id) return s and (s.subtext or "") end)
    set("C_Spell.GetSpellDescription", function(id) local s = findSpell(id) return s and (s.description or "") end)
    set("C_Spell.GetSpellLink", function(id) local s = findSpell(id) return s and link(s) end)
    set("C_Spell.GetSpellIDForSpellIdentifier", function(id) local s = findSpell(id) return s and s.id end)
    set("C_Spell.GetOverrideSpell", function(id) local s = findSpell(id) return s and s.id or id end)
    set("C_Spell.GetBaseSpell", function(id) local s = findSpell(id) return s and s.id or id end)
    set("C_Spell.IsSpellPassive", function(id) local s = findSpell(id) return s and s.passive == true end)
    set("C_Spell.IsSpellHarmful", function(id)
        local s = findSpell(id)
        return s ~= nil and (s.harmful or (s.aura and s.aura.harmful)) == true
    end)
    set("C_Spell.IsSpellHelpful", function(id)
        local s = findSpell(id)
        return s ~= nil and not (s.harmful or (s.aura and s.aura.harmful))
    end)
    set("C_Spell.GetSpellCooldown", function(id)
        local s = findSpell(id)
        if s then return spellCooldown(s) end
        return cooldownInfo(0, 0)
    end)
    set("C_Spell.GetSpellCharges", function() return nil end)
    set("C_Spell.GetSpellCastCount", function() return 0 end)
    set("C_Spell.GetSpellDisplayCount", function() return "" end)
    set("C_Spell.GetSpellPowerCost", function(id) local s = findSpell(id) return s and powerCost(s) end)
    set("C_Spell.IsSpellUsable", function(id) return usable(findSpell(id)) end)
    set("C_Spell.IsCurrentSpell", function(id)
        local s = findSpell(id)
        return s ~= nil and cast ~= nil and cast.spell == s
    end)
    set("C_Spell.SpellHasRange", function(id) local s = findSpell(id) return s ~= nil and (s.range or 0) > 0 end)
    set("C_Spell.IsSpellInRange", function(id, unit)
        return inRange(findSpell(id), resolveUnit(unit or "target"))
    end)
    set("C_Spell.IsSpellDataCached", function() return true end)
    set("C_Spell.RequestLoadSpellData", function() end)
    set("C_Spell.GetSpellLossOfControlCooldown", function() return 0, 0 end)
    set("C_Spell.GetSpellLossOfControlCooldownInfo", function() return nil end)
    set("C_Spell.PickupSpell", function(id)
        local s = findSpell(id)
        if s and not s.passive then setCursor({ type = "spell", id = s.id }) end
    end)
    set("C_Spell.IsAutoAttackSpell", function() return false end)
    set("C_Spell.IsRangedAutoAttackSpell", function() return false end)
    set("C_Spell.IsPressHoldReleaseSpell", function() return false end)
    set("C_Spell.GetSpellMaxCumulativeAuraApplications", function(id)
        local s = findSpell(id)
        return s and s.aura and s.aura.maxStacks or 0
    end)
    set("IsSpellKnown", function(id) return findSpell(id) ~= nil end)
    set("IsPlayerSpell", function(id) return findSpell(id) ~= nil end)
    set("C_SpellBook.IsSpellKnown", function(id) return findSpell(id) ~= nil end)
    set("C_SpellBook.IsSpellInSpellBook", function(id) return findSpell(id) ~= nil end)

    -- Spellbook.
    set("C_SpellBook.GetNumSpellBookSkillLines", function() return #skillLines end)
    set("C_SpellBook.GetSpellBookSkillLineInfo", function(i)
        local line = skillLines[i]
        if not line then return nil end
        return { name = line.name, iconID = line.icon, itemIndexOffset = line.offset,
            numSpellBookItems = #line.spells, isGuild = false, shouldHide = false,
            specID = line.specID }
    end)
    set("C_SpellBook.GetSkillLineIndexByID", function() return nil end)
    set("C_SpellBook.HasPetSpells", function() return nil end)
    set("C_SpellBook.GetSpellBookItemInfo", function(slot, bank)
        local s = bookSpell(slot, bank)
        if not s then
            return { actionID = 0, itemType = Enum.SpellBookItemType.None, name = "",
                subName = "", iconID = 0, isPassive = false, isOffSpec = false }
        end
        return { actionID = s.id, spellID = s.id, itemType = Enum.SpellBookItemType.Spell,
            name = s.name, subName = s.subtext or "", iconID = s.icon,
            isPassive = s.passive == true, isOffSpec = false, skillLineIndex = lineOf(slot) }
    end)
    set("C_SpellBook.GetSpellBookItemName", function(slot, bank)
        local s = bookSpell(slot, bank)
        if s then return s.name, s.subtext or "" end
    end)
    set("C_SpellBook.GetSpellBookItemTexture", function(slot, bank)
        local s = bookSpell(slot, bank)
        return s and s.icon
    end)
    set("C_SpellBook.GetSpellBookItemType", function(slot, bank)
        local s = bookSpell(slot, bank)
        if not s then return Enum.SpellBookItemType.None, 0, nil end
        return Enum.SpellBookItemType.Spell, s.id, s.id
    end)
    set("C_SpellBook.IsSpellBookItemPassive", function(slot, bank)
        local s = bookSpell(slot, bank)
        return s ~= nil and s.passive == true
    end)
    set("C_SpellBook.IsSpellBookItemOffSpec", function() return false end)
    set("C_SpellBook.GetSpellBookItemCooldown", function(slot, bank)
        local s = bookSpell(slot, bank)
        return s and spellCooldown(s) or cooldownInfo(0, 0)
    end)
    set("C_SpellBook.GetSpellBookItemCharges", function() return nil end)
    set("C_SpellBook.GetSpellBookItemCastCount", function() return 0 end)
    set("C_SpellBook.GetSpellBookItemDescription", function(slot, bank)
        local s = bookSpell(slot, bank)
        return s and s.description or ""
    end)
    set("C_SpellBook.GetSpellBookItemLink", function(slot, bank)
        local s = bookSpell(slot, bank)
        return s and link(s)
    end)
    set("C_SpellBook.GetSpellBookItemLevelLearned", function(slot, bank)
        local s = bookSpell(slot, bank)
        return s and s.level or 1
    end)
    set("C_SpellBook.GetSpellBookItemAutoCast", function() return false, false end)
    set("C_SpellBook.GetSpellBookItemPowerCost", function(slot, bank)
        local s = bookSpell(slot, bank)
        return s and powerCost(s) or {}
    end)
    set("C_SpellBook.GetSpellBookItemLossOfControlCooldown", function() return 0, 0 end)
    set("C_SpellBook.IsAutoAttackSpellBookItem", function() return false end)
    set("C_SpellBook.IsRangedAutoAttackSpellBookItem", function() return false end)
    set("C_SpellBook.FindSpellBookSlotForSpell", function(id)
        local s = findSpell(id)
        if not s then return nil end
        for slot, sid in pairs(book) do
            if sid == s.id then return slot, Enum.SpellBookSpellBank.Player end
        end
    end)
    set("C_SpellBook.FindBaseSpellByID", function(id) return id end)
    set("C_SpellBook.FindSpellOverrideByID", function(id) return id end)
    set("C_SpellBook.PickupSpellBookItem", function(slot, bank)
        local s = bookSpell(slot, bank)
        if s and not s.passive then setCursor({ type = "spell", id = s.id, slot = slot }) end
    end)
    set("C_SpellBook.CastSpellBookItem", function(slot, bank, targetSelf)
        castSpell(bookSpell(slot, bank), targetSelf and "player" or nil)
    end)

    -- Cursor.
    set("GetCursorInfo", function()
        if not cursor then return nil end
        if cursor.type == "spell" then
            return "spell", cursor.slot or 0, "spell", cursor.id, cursor.id
        elseif cursor.type == "action" then
            if cursor.actionType == "spell" then
                return "spell", 0, "spell", cursor.id, cursor.id
            end
            return cursor.actionType, cursor.id
        elseif cursor.type == "macro" then
            return "macro", cursor.id
        end
    end)
    set("CursorHasSpell", function() return cursor ~= nil and (cursor.type == "spell" or cursor.actionType == "spell") end)
    set("CursorHasMacro", function() return cursor ~= nil and (cursor.type == "macro" or cursor.actionType == "macro") end)
    set("CursorHasItem", function() return false end)
    set("ClearCursor", function() if cursor then setCursor(nil) end end)
    set("C_Cursor.GetCursorItem", function() return nil end)

    -- Action slots.
    local function hasAction(slot) return actions[slot] ~= nil end
    set("HasAction", hasAction)
    set("C_ActionBar.HasAction", hasAction)
    set("GetActionInfo", function(slot)
        local a = actions[slot]
        if a then return a.type, a.id, a.type == "spell" and "spell" or nil end
    end)
    set("C_ActionBar.GetActionTexture", function(slot)
        local m = actionMacro(slot)
        if m then return macroIcon(m) end
        local s = actionSpell(slot)
        return s and s.icon
    end)
    set("GetActionTexture", C_ActionBar.GetActionTexture)
    -- A macro's name shows on its button.
    set("C_ActionBar.GetActionText", function(slot)
        local m = actionMacro(slot)
        return m and m.name
    end)
    set("GetActionText", C_ActionBar.GetActionText)
    set("C_ActionBar.UsesActionText", function(slot) return actionMacro(slot) ~= nil end)
    set("C_ActionBar.GetActionDisplayCount", function() return "" end)
    set("C_ActionBar.GetActionUseCount", function() return 0 end)
    set("C_ActionBar.IsUsableAction", function(slot)
        if actionMacro(slot) and not actionSpell(slot) then return true, false end
        return usable(actionSpell(slot))
    end)
    set("C_ActionBar.GetActionCooldown", function(slot)
        local s = actionSpell(slot)
        return s and spellCooldown(s) or cooldownInfo(0, 0)
    end)
    set("C_ActionBar.GetActionCharges", function()
        return { currentCharges = 0, maxCharges = 0, cooldownStartTime = 0,
            cooldownDuration = 0, chargeModRate = 1, isActive = false }
    end)
    set("C_ActionBar.GetActionLossOfControlCooldownInfo", function() return nil end)
    set("C_ActionBar.IsCurrentAction", function(slot)
        local s = actionSpell(slot)
        return s ~= nil and cast ~= nil and cast.spell == s
    end)
    set("C_ActionBar.GetSpell", function(slot) local s = actionSpell(slot) return s and s.id end)
    set("C_ActionBar.FindSpellActionButtons", function(id)
        local s = findSpell(id)
        local out = {}
        if s then
            for slot, a in pairs(actions) do
                if a.type == "spell" and a.id == s.id then out[#out + 1] = slot end
            end
            table.sort(out)
        end
        return out
    end)
    set("C_ActionBar.IsOnBarOrSpecialBar", function(id)
        return #C_ActionBar.FindSpellActionButtons(id) > 0
    end)
    for _, name in ipairs({ "IsAttackAction", "IsAutoRepeatAction", "IsConsumableAction",
            "IsItemAction", "IsEquippedAction", "IsStackableAction", "IsAssistedCombatAction",
            "IsEquippedGearOutfitAction", "IsAutoCastPetAction", "IsEnabledAutoCastPetAction" }) do
        set("C_ActionBar." .. name, function() return false end)
    end
    set("C_ActionBar.HasRangeRequirements", function(slot)
        local s = actionSpell(slot)
        return s ~= nil and (s.range or 0) > 0
    end)
    set("C_ActionBar.EnableActionRangeCheck", function(slot, enable)
        local w = rangeWatch[slot]
        if enable then
            if not w then
                -- Nothing sent yet, so the first check reports the state.
                w = { count = 0 }
                rangeWatch[slot] = w
            end
            w.count = w.count + 1
        elseif w then
            w.count = w.count - 1
            if w.count <= 0 then rangeWatch[slot] = nil end
        end
    end)
    set("C_ActionBar.IsActionInRange", function(slot, unit)
        local s = actionSpell(slot)
        if not s or (s.range or 0) <= 0 then return nil end
        local u = unit and resolveUnit(unit) or spellTarget(s)
        if not u then return nil end
        return inRange(s, u)
    end)
    set("PickupAction", function(slot)
        local a = actions[slot]
        if not a then return end
        -- Picking up while holding something swaps the two.
        local held = cursor
        if held then
            setAction(slot, { type = held.type == "action" and held.actionType or held.type, id = held.id })
        else
            setAction(slot, nil)
        end
        setCursor({ type = "action", slot = slot, id = a.id, actionType = a.type })
    end)
    set("PlaceAction", placeAction)
    set("C_ActionBar.PutActionInSlot", placeAction)
    set("UseAction", function(slot, unit)
        if cursor then return placeAction(slot) end
        local a = actions[slot]
        if a and a.type == "macro" then return RunMacro(a.id) end
        local s = actionSpell(slot)
        if s then castSpell(s, unit) end
    end)

    -- Macros.
    set("GetNumMacros", function()
        local account, character = 0, 0
        for i in pairs(macros) do
            if i > MAX_ACCOUNT_MACROS then character = character + 1 else account = account + 1 end
        end
        return account, character
    end)
    set("GetMacroInfo", function(id)
        local m = findMacro(id)
        if m then return m.name, macroIcon(m), m.body end
    end)
    set("GetMacroIndexByName", function(name)
        local _, i = findMacro(name)
        return i or 0
    end)
    set("GetMacroBody", function(id) local m = findMacro(id) return m and m.body end)
    set("GetMacroSpell", function(id)
        local s = macroSpell((findMacro(id)))
        return s and s.id
    end)
    set("GetMacroItem", function() return nil end)
    set("RunMacro", function(id)
        local m = findMacro(id)
        if m then RunMacroText(m.body) end
    end)
    set("PickupMacro", function(id)
        local m, i = findMacro(id)
        if m then setCursor({ type = "macro", id = i }) end
    end)

    -- Casting.
    set("CastSpellByID", function(id, unit) castSpell(findSpell(id), unit) end)
    set("CastSpellByName", function(name, unit) castSpell(findSpell(name), unit) end)
    set("SpellStopCasting", function()
        if not cast then return false end
        finishCast(false)
        return true
    end)
    set("SpellIsTargeting", function() return false end)
    set("SpellStopTargeting", function() return false end)
    set("SpellCanTargetUnit", function() return false end)
    local function castingInfo(unit)
        local u = unitData(unit)
        if not cast or cast.channel or u ~= player() then return nil end
        local s = cast.spell
        return s.name, s.name, s.icon, cast.start * 1000, cast.finish * 1000, false,
            cast.guid, false, s.id, nil, 0
    end
    local function channelInfo(unit)
        local u = unitData(unit)
        if not cast or not cast.channel or u ~= player() then return nil end
        local s = cast.spell
        return s.name, s.name, s.icon, cast.start * 1000, cast.finish * 1000, false,
            false, s.id, false, 0, nil
    end
    set("UnitCastingInfo", castingInfo)
    set("UnitChannelInfo", channelInfo)

    -- Target.
    set("TargetUnit", function(name, exact)
        local u = resolveUnit(name, exact)
        if u then setTarget(u) end
    end)
    set("ClearTarget", function()
        local had = units.target ~= nil
        setTarget(nil)
        return had
    end)
    -- Tab: living hostile units within 40 yards, nearest first; pressing
    -- again moves to the next one (units without a position come last).
    local TAB_RANGE = 40
    local function nearestEnemy()
        local list, seen = {}, {}
        for _, group in ipairs({ hidden, units }) do
            for _, u in pairs(group) do
                if not seen[u] and hostile(u) and not u.dead then
                    seen[u] = true
                    local d = distance(player(), u) or math.huge
                    if d <= TAB_RANGE or d == math.huge then list[#list + 1] = { u = u, d = d } end
                end
            end
        end
        table.sort(list, function(a, b)
            if a.d ~= b.d then return a.d < b.d end
            return (a.u.name or "") < (b.u.name or "")
        end)
        if #list == 0 then return end
        local pick = 1
        for i, e in ipairs(list) do
            if e.u == units.target then pick = i % #list + 1 end
        end
        setTarget(list[pick].u)
    end
    set("TargetNearestEnemy", nearestEnemy)
    set("TargetNearest", nearestEnemy)

    -- Auras.
    local function byIndex(unit, index, filter)
        local u = unitData(unit)
        if not u then return nil end
        local a = filtered(u, filter)[index]
        return a and auraData(a)
    end
    set("C_UnitAuras.GetAuraDataByIndex", byIndex)
    set("C_UnitAuras.GetBuffDataByIndex", function(unit, index, filter)
        return byIndex(unit, index, "HELPFUL" .. (filter and ("|" .. filter) or ""))
    end)
    set("C_UnitAuras.GetDebuffDataByIndex", function(unit, index, filter)
        return byIndex(unit, index, "HARMFUL" .. (filter and ("|" .. filter) or ""))
    end)
    set("C_UnitAuras.GetAuraSlots", function(unit, filter)
        local u = unitData(unit)
        if not u then return nil end
        local out = {}
        for _, a in ipairs(filtered(u, filter)) do out[#out + 1] = a.auraInstanceID end
        return nil, unpack(out)
    end)
    -- Slots are aura instance IDs (inferred: both are opaque numbers).
    set("C_UnitAuras.GetAuraDataBySlot", function(unit, slot)
        local u = unitData(unit)
        for _, a in ipairs(u and auras[u] or {}) do
            if a.auraInstanceID == slot then return auraData(a) end
        end
    end)
    set("C_UnitAuras.GetAuraDataByAuraInstanceID", function(unit, id)
        local u = unitData(unit)
        for _, a in ipairs(u and auras[u] or {}) do
            if a.auraInstanceID == id then return auraData(a) end
        end
    end)
    set("C_UnitAuras.GetUnitAuras", function(unit, filter, maxCount)
        local u = unitData(unit)
        local out = {}
        for _, a in ipairs(u and filtered(u, filter) or {}) do
            if maxCount and #out >= maxCount then break end
            out[#out + 1] = auraData(a)
        end
        return out
    end)
    local function bySpellID(unit, id)
        local u = unitData(unit)
        for _, a in ipairs(u and auras[u] or {}) do
            if a.spellId == id then return auraData(a) end
        end
    end
    set("C_UnitAuras.GetUnitAuraBySpellID", bySpellID)
    set("C_UnitAuras.GetPlayerAuraBySpellID", function(id) return bySpellID("player", id) end)
    set("C_UnitAuras.IsAuraFilteredOutByInstanceID", function(unit, id, filter)
        local u = unitData(unit)
        for _, a in ipairs(u and auras[u] or {}) do
            if a.auraInstanceID == id then return not filterMatches(a, filter) end
        end
        return true
    end)
    set("C_UnitAuras.CancelAuraByAuraInstanceID", function(unit, id)
        local u = unitData(unit)
        if u then removeAura(u, id) end
    end)
    set("CancelUnitBuff", function(unit, index, filter)
        local u = unitData(unit)
        if not u then return end
        local a = filtered(u, "HELPFUL" .. (filter and ("|" .. filter) or ""))[index]
        if a then removeAura(u, a.auraInstanceID) end
    end)
    set("CancelSpellByName", function(name)
        local s = findSpell(name)
        local u = player()
        for _, a in ipairs(s and auras[u] or {}) do
            if a.spellId == s.id and a.isHelpful then removeAura(u, a.auraInstanceID) return end
        end
    end)

    -- Unit tokens from GUIDs: the first token that names the unit.
    set("UnitTokenFromGUID", function(guid)
        if guid == nil then return nil end
        for _, token in ipairs({ "player", "target", "focus", "mouseover", "pet",
                "party1", "party2", "party3", "party4" }) do
            local u = units[token]
            if u and u.guid == guid then return token end
        end
        for token, u in pairs(extraTokens) do
            if u.guid == guid then return token end
        end
    end)

    -- Combat.
    set("InCombatLockdown", function() return lockdown end)
    set("UnitAffectingCombat", function(unit)
        local u = resolveUnit(unit)
        return inCombat and u ~= nil and (u == player() or u.canAttack == true)
    end)

    installCombatLog()
    installWorld()
    set("C_CombatText.GetCurrentEventInfo", function()
        return combatTextInfo[1], combatTextInfo[2], combatTextInfo[3]
    end)
    set("C_CombatText.SetActiveUnit", function(unit) combatTextUnit = unit or "player" end)

    -- Tooltips.
    for name, fn in pairs(State.tooltipFunctions) do set("C_TooltipInfo." .. name, fn) end

    local driver = CreateFrame("Frame")
    driver:SetScript("OnUpdate", function(_, elapsed) tick(elapsed) end)
end

local function enable()
    if enabled then return end
    enabled = true
    -- Action slots the player arranged are kept for the character.
    actions = {}
    install()
end

-- Scenario functions --------------------------------------------------------------

-- State.AddSpell(def) adds a known spell. Fields: id, name, icon (file ID
-- or path), subtext, description, castTime and channel (seconds), cooldown
-- (seconds), gcd (seconds, default 1.5; 0 for none), cost (power), restore
-- (power given back), range (yards), harmful, passive, level, and aura =
-- { harmful, duration, maxStacks, dispelName, self, description }.
function State.AddSpell(def)
    enable()
    local s = {}
    for k, v in pairs(def) do s[k] = v end
    s.icon = iconID(def.icon)
    spells[s.id] = s
    spellByName[s.name:lower()] = s
    return s.id
end

-- State.AddSkillLine{ name, icon, specID, spells = { def or id, ... } }
-- adds a spellbook tab section.
function State.AddSkillLine(def)
    enable()
    local line = { name = def.name, icon = iconID(def.icon), specID = def.specID, spells = {} }
    for _, sp in ipairs(def.spells or {}) do
        local id = type(sp) == "table" and State.AddSpell(sp) or sp
        line.spells[#line.spells + 1] = id
    end
    skillLines[#skillLines + 1] = line
    rebuildBook()
end

-- State.SetDefaultActions{ [slot] = spell ID or { macro = name }, ... }: the bar a new
-- character starts with. Slots the player changed are kept instead.
function State.SetDefaultActions(list)
    enable()
    local saved = _G[SAVED]
    for slot, id in pairs(list) do
        if type(id) == "table" and id.macro then
            local _, i = findMacro(id.macro)
            if i then actions[slot] = { type = "macro", id = i } end
        else
            actions[slot] = { type = "spell", id = id }
        end
    end
    if type(saved) == "table" then
        for slot, a in pairs(saved) do
            if a == false then
                actions[slot] = nil
            elseif type(a) == "table" then
                actions[slot] = { type = a[1], id = a[2] }
            end
        end
    end
end

-- State.AddMacro{ name, body, icon, perCharacter } adds a macro and
-- returns its index. Without an icon, the macro shows its spell's icon.
function State.AddMacro(def)
    enable()
    local first = def.perCharacter and MAX_ACCOUNT_MACROS + 1 or 1
    local i = first
    while macros[i] do i = i + 1 end
    macros[i] = { name = def.name, body = def.body or "", icon = def.icon and iconID(def.icon) }
    return i
end

-- State.AddHiddenUnit(key, data): a unit near the player that has no unit
-- token until targeted, such as a training dummy (fields as S.SetUnit).
function State.AddHiddenUnit(key, data)
    enable()
    data.token = key
    hidden[key] = data
    return data
end

-- State.PlaceUnit(unit, x, y): put a unit at a world position (yards on
-- the player's continent, x north, y west, as WowProgrammingWorld).
function State.PlaceUnit(unit, x, y)
    local u = hidden[unit] or resolveUnit(unit)
    if u then u.x, u.y = x, y end
end

-- State.ApplyAura(unit, spellID, aura) puts an aura on a unit, as a
-- scenario setup (a debuff from another source, for example).
function State.ApplyAura(unit, id, def)
    local u = resolveUnit(unit)
    local s = findSpell(id) or { id = id, name = def.name or ("Spell " .. id), icon = iconID(def.icon) }
    if u then return applyAura(u, s, def) end
end

-- State.EnterCombat() and State.LeaveCombat(), for scenarios and specs.
-- Combat entered this way still ends after COMBAT_TIMEOUT seconds without
-- a hostile action unless State.EnterCombat is called again.
function State.EnterCombat() enterCombat() end
function State.LeaveCombat() leaveCombat() end
function State.InCombat() return inCombat end

function State.Cursor() return cursor end
function State.Casting() return cast end
function State.Action(slot) return actions and actions[slot] end
function State.Auras(unit) local u = resolveUnit(unit) return u and auras[u] or {} end

return State
