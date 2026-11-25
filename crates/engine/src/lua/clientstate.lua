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
    for _, e in ipairs(q) do fireNow(unpack(e, 1, e.n)) end
end
local function fire(...)
    queue[#queue + 1] = { n = select("#", ...), ... }
    if not flushing then
        flushing = true
        C_Timer.After(0, flush)
    end
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
    copy.expires = nil
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
    list[#list + 1] = a
    fireAura(u, { isFullUpdate = false, addedAuras = { auraData(a) } })
    return a
end

local function removeAura(u, instanceID)
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

-- Casting ---------------------------------------------------------------------

local function castGUID()
    local guid = string.format("Cast-3-0-0-0-%d-%010d", 0, nextCastID)
    nextCastID = nextCastID + 1
    return guid
end

local function spellTarget(s, unit)
    if unit then return resolveUnit(unit) end
    if s.harmful then return units.target end
    if s.aura and s.aura.harmful then return units.target end
    return player()
end

-- The spell's effects when it lands: costs are paid at the start (inferred),
-- auras applied here.
local function land(s, target)
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
    if cast then return failCast(s, SPELL_FAILED_SPELL_IN_PROGRESS) end
    local cd = spellCooldown(s)
    if cd.isActive then
        return failCast(s, cd.isOnGCD and SPELL_FAILED_NOT_READY or SPELL_FAILED_NOT_READY)
    end
    local ok, noPower = usable(s)
    if not ok then return failCast(s, noPower and ERR_OUT_OF_MANA or nil) end
    local target = spellTarget(s, unit)
    if (s.harmful or (s.aura and s.aura.harmful)) and not target then
        return failCast(s, ERR_GENERIC_NO_TARGET or SPELL_FAILED_BAD_TARGETS)
    end
    local u = player()
    local guid = castGUID()
    if s.harmful or (s.aura and s.aura.harmful) then enterCombat() end
    fire("UNIT_SPELLCAST_SENT", "player", target and target.name or "", guid, s.id)
    if s.cost then setPower(u, (u.power or 0) - s.cost) end
    startCooldowns(s)
    local t = now()
    if (s.channel or 0) > 0 then
        cast = { spell = s, guid = guid, start = t, finish = t + s.channel, channel = true, target = target }
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
local gcdWasActive = false
local function tick(elapsed)
    local t = now()
    if inCombat and t - lastHostile > COMBAT_TIMEOUT then leaveCombat() end
    if cast and t >= cast.finish then finishCast(true) end
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

local function set(path, fn)
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
    set("C_Spell.IsSpellInRange", function(id) local s = findSpell(id) if s and (s.range or 0) > 0 then return true end end)
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
    set("C_ActionBar.IsActionInRange", function(slot)
        local s = actionSpell(slot)
        if s and (s.range or 0) > 0 then
            if s.harmful or (s.aura and s.aura.harmful) then return units.target ~= nil end
            return true
        end
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
    local function nearestEnemy()
        for _, list in ipairs({ hidden, units }) do
            for key, u in pairs(list) do
                if key ~= "target" and u.canAttack and u ~= units.target then
                    setTarget(u)
                    return
                end
            end
        end
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
    end)

    -- Combat.
    set("InCombatLockdown", function() return lockdown end)
    set("UnitAffectingCombat", function(unit)
        local u = resolveUnit(unit)
        return inCombat and u ~= nil and (u == player() or u.canAttack == true)
    end)

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
