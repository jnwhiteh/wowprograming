-- Macro and slash command execution, as the C++ client provides it.
--
-- The client runs every macro line (RunMacroText, RunMacro, a macro on an
-- action bar) through one callback that Blizzard_ChatFrameBase registers
-- with C_Macro.SetMacroExecuteLineCallback (MacroExecutionManager.lua).
-- The callback puts the line into a private edit box and sends it, so
-- Blizzard's chat parser runs secure commands (/cast, /target,
-- /stopcasting, /logout) and SlashCmdList commands alike.
--
-- SecureCmdOptionParse evaluates macro options ("[@focus,harm] Spell;
-- Other"). The conditions the sandbox can answer are below; unknown
-- conditions are false (and true with "no"). See docs/client-runtime.md.
local type, pairs, ipairs, tostring, tonumber, select, string, table =
      type, pairs, ipairs, tostring, tonumber, select, string, table

local executeLine -- Blizzard's callback

C_Macro = C_Macro or {}
function C_Macro.SetMacroExecuteLineCallback(fn) executeLine = fn end

local function trim(s) return (s:gsub("^%s+", ""):gsub("%s+$", "")) end

-- Run each line of a macro body. Lines starting with "#" (#showtooltip)
-- are instructions for the client, not commands.
function RunMacroText(text)
    if type(text) ~= "string" or not executeLine then return end
    for raw in (text .. "\n"):gmatch("([^\r\n]*)\r?\n") do
        local line = trim(raw)
        if line ~= "" and line:sub(1, 1) ~= "#" then executeLine(line) end
    end
end

-- The console (and the GUI's console) runs a slash command it does not
-- know itself through Blizzard's parser. Returns false when the chat
-- frame is not loaded or Blizzard does not know the command, so the
-- console can say so.
function WowProgramming_ExecuteChatLine(line)
    if not executeLine then return false end
    local command = (line:match("^(/[^%s]+)") or ""):upper()
    local known = (rawget(_G, "hash_ChatTypeInfoList") or {})[command]
        or (rawget(_G, "hash_SlashCmdList") or {})[command]
        or (rawget(_G, "hash_EmoteTokenList") or {})[command]
    if not known then return false end
    executeLine(line)
    return true
end

-- Macro options -----------------------------------------------------------------

local function unitFor(unit) return unit or "target" end

local function playerCasting()
    return UnitCastingInfo and UnitCastingInfo("player") ~= nil
end

local function playerChanneling()
    return UnitChannelInfo and UnitChannelInfo("player") ~= nil
end

-- name -> function(unit, args) for the conditions the sandbox knows.
local conditions = {
    exists = function(unit) return UnitExists(unitFor(unit)) == true end,
    help = function(unit)
        local u = unitFor(unit)
        return UnitExists(u) == true and not UnitCanAttack("player", u)
    end,
    harm = function(unit)
        local u = unitFor(unit)
        return UnitExists(u) == true and UnitCanAttack("player", u) == true
    end,
    dead = function(unit) return UnitIsDead(unitFor(unit)) == true end,
    combat = function() return InCombatLockdown() == true end,
    mod = function() return IsModifierKeyDown() == true end,
    modifier = function() return IsModifierKeyDown() == true end,
    channeling = function() return playerChanneling() end,
    casting = function() return playerCasting() end,
    -- Clicks from the sandbox's macro paths are the left button.
    button = function(_, args)
        if not args then return true end
        for _, a in ipairs(args) do
            if a == "1" or a:lower() == "leftbutton" then return true end
        end
        return false
    end,
    known = function(_, args)
        for _, a in ipairs(args or {}) do
            local id = tonumber(a) or (C_Spell.GetSpellIDForSpellIdentifier
                and C_Spell.GetSpellIDForSpellIdentifier(a))
            if id and IsPlayerSpell(id) then return true end
        end
        return false
    end,
    group = function() return IsInGroup and IsInGroup() == true end,
    pet = function() return UnitExists("pet") == true end,
    -- The player stands on the ground, outdoors, unmounted, in no form.
    indoors = function() return false end,
    outdoors = function() return true end,
    mounted = function() return false end,
    flying = function() return false end,
    swimming = function() return false end,
    stealth = function() return false end,
    form = function(_, args)
        if not args then return false end
        for _, a in ipairs(args) do if a == "0" then return true end end
        return false
    end,
}
conditions.stance = conditions.form

-- One condition: "harm", "nodead", "mod:shift", "@focus", "target=focus".
-- Returns ok, unit (when the condition names the unit).
local function evaluate(cond, unit)
    if cond == "" then return true end
    local at = cond:match("^@(.+)$") or cond:match("^target%s*=%s*(.+)$")
    if at then return true, at end
    local name, rest = cond:match("^([%a]+)%s*:?%s*(.*)$")
    if not name then return false end
    name = name:lower()
    local negate = false
    if name:sub(1, 2) == "no" and not conditions[name] then
        negate, name = true, name:sub(3)
    end
    local args
    if rest ~= "" then
        args = {}
        for a in rest:gmatch("[^/]+") do args[#args + 1] = trim(a) end
    end
    local fn = conditions[name]
    local ok = fn ~= nil and fn(unit, args) == true
    if negate then ok = not ok end
    return ok
end

-- One bracket group "[a,b,c]": every condition holds. The unit comes
-- first, whatever its position in the group.
local function groupMatches(group)
    local parts = {}
    for c in (group .. ","):gmatch("([^,]*),") do parts[#parts + 1] = trim(c) end
    local unit
    for _, c in ipairs(parts) do
        local u = c:match("^@(.+)$") or c:match("^target%s*=%s*(.+)$")
        if u then unit = trim(u) end
    end
    for _, c in ipairs(parts) do
        if not evaluate(c, unit) then return false end
    end
    return true, unit
end

-- SecureCmdOptionParse("[harm] A; [@player] B; C") returns the action of
-- the first clause whose conditions hold, and its target unit.
function SecureCmdOptionParse(msg)
    if type(msg) ~= "string" then return nil end
    for clause in (msg .. ";"):gmatch("([^;]*);") do
        clause = trim(clause)
        local groups = {}
        local rest = clause
        while true do
            local g, r = rest:match("^%s*(%b[])(.*)$")
            if not g then break end
            groups[#groups + 1] = g:sub(2, -2)
            rest = r
        end
        local action = trim(rest)
        if #groups == 0 then return action, nil end
        for _, g in ipairs(groups) do
            local ok, unit = groupMatches(g)
            if ok then return action, unit end
        end
    end
    return nil
end
