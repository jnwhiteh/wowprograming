-- Key bindings, as the C++ client keeps them.
--
-- A key runs, in order: the newest priority override binding, the newest
-- other override binding, then the key's normal binding (inferred from
-- the SetOverrideBinding documentation). A command is a binding action
-- from Bindings_Standard.xml ("MOVEFORWARD"), "CLICK Name:Button",
-- "SPELL name", "MACRO name" or "ITEM name". Actions marked runOnUp get
-- "up" when the key is released; a CLICK binding clicks the button down
-- and up as its click registrations ask.
--
-- Called with the binding actions (name -> function(keystate)), the
-- runOnUp set, a check for click registrations, a function that fires an
-- event and one that saves the bindings file. Returns run(key, down) and
-- apply(fileText).
local actions, runOnUp, registeredFor, fire, saveFile = ...
local type, pairs, ipairs, tostring, table, string, select, unpack =
      type, pairs, ipairs, tostring, table, string, select, unpack

-- The client's default keys for the actions the sandbox supports.
local DEFAULTS = {
    W = "MOVEFORWARD", S = "MOVEBACKWARD", A = "TURNLEFT", D = "TURNRIGHT",
    Q = "STRAFELEFT", E = "STRAFERIGHT", UP = "MOVEFORWARD", DOWN = "MOVEBACKWARD",
    LEFT = "TURNLEFT", RIGHT = "TURNRIGHT", SPACE = "JUMP", NUMLOCK = "TOGGLEAUTORUN",
    P = "TOGGLESPELLBOOK", ["SHIFT-P"] = "TOGGLEPETBOOK", N = "TOGGLETALENTS",
    C = "TOGGLECHARACTER0", B = "OPENALLBAGS", M = "TOGGLEWORLDMAP",
    ESCAPE = "TOGGLEGAMEMENU", TAB = "TARGETNEARESTENEMY",
    ["1"] = "ACTIONBUTTON1", ["2"] = "ACTIONBUTTON2", ["3"] = "ACTIONBUTTON3",
    ["4"] = "ACTIONBUTTON4", ["5"] = "ACTIONBUTTON5", ["6"] = "ACTIONBUTTON6",
    ["7"] = "ACTIONBUTTON7", ["8"] = "ACTIONBUTTON8", ["9"] = "ACTIONBUTTON9",
    ["0"] = "ACTIONBUTTON10", ["-"] = "ACTIONBUTTON11", ["="] = "ACTIONBUTTON12",
}

local bindings = {}            -- key -> command
for k, v in pairs(DEFAULTS) do bindings[k] = v end
local overrides = {}           -- list of { owner, priority, key, command }, oldest first
local held = {}                -- key -> command started by the key press

-- Keys are upper case, with modifiers in the client's order.
local function normalize(key)
    if type(key) ~= "string" then return nil end
    local mods, rest = {}, key:upper()
    for _, m in ipairs({ "ALT", "CTRL", "SHIFT", "META" }) do
        local found
        rest, found = rest:gsub("^(.-)" .. m .. "%-(.+)$", "%1%2")
        if found > 0 then mods[#mods + 1] = m end
    end
    if #mods == 0 then return rest end
    return table.concat(mods, "-") .. "-" .. rest
end

local updatePending = false
local function bindingsChanged()
    if updatePending then return end
    updatePending = true
    C_Timer.After(0, function()
        updatePending = false
        fire("UPDATE_BINDINGS")
    end)
end

local function resolve(key)
    for pass = 1, 2 do
        for i = #overrides, 1, -1 do
            local o = overrides[i]
            if o.key == key and (pass == 2 or o.priority) then return o.command end
        end
    end
    return bindings[key]
end

local function clickFrame(command, down)
    local name, button = command:match("^CLICK%s+([^:]+):?(.*)$")
    local frame = name and _G[name]
    if type(frame) ~= "table" or not frame.Click then return end
    button = (button ~= "" and button) or "LeftButton"
    if registeredFor(frame, button, down) then frame:Click(button, down) end
end

-- Run a command for a key going down or up. Returns whether it ran.
local function runCommand(command, down)
    local kind, arg = command:match("^(%u+)%s+(.+)$")
    if kind == "CLICK" then
        clickFrame(command, down)
        return true
    elseif kind == "SPELL" then
        if down then CastSpellByName(arg) end
        return true
    elseif kind == "MACRO" then
        if down then RunMacro(arg) end
        return true
    elseif kind == "ITEM" then
        if down and C_Item and C_Item.UseItemByName then C_Item.UseItemByName(arg) end
        return true
    end
    if command == "TOGGLEGAMEMENU" and not rawget(_G, "ToggleGameMenu") then
        -- Without the game menu addon, Escape closes open panels.
        if down and CloseAllWindows then CloseAllWindows() end
        return true
    end
    local f = actions[command]
    if not f then return false end
    if down then
        f("down")
    elseif runOnUp[command] then
        f("up")
    end
    return true
end

local function run(key, down)
    key = normalize(key)
    if not key then return false end
    if down then
        local command = resolve(key)
        if not command or command == "" then return false end
        held[key] = command
        return runCommand(command, true)
    end
    local command = held[key]
    held[key] = nil
    if not command then return false end
    return runCommand(command, false)
end

-- Binding API ---------------------------------------------------------------------

function GetBindingKey(command)
    local keys = {}
    for k, v in pairs(bindings) do
        if v == command then keys[#keys + 1] = k end
    end
    table.sort(keys)
    return unpack(keys)
end

function GetBindingAction(key, checkOverride)
    key = normalize(key)
    if not key then return "" end
    if checkOverride then return resolve(key) or "" end
    return bindings[key] or ""
end

function GetBindingByKey(key)
    return GetBindingAction(key, true)
end

function GetBindingText(key)
    if type(key) ~= "string" then return "" end
    return key
end

function SetBinding(key, command)
    key = normalize(key)
    if not key then return false end
    bindings[key] = command
    bindingsChanged()
    return true
end

function SetBindingClick(key, buttonName, mouseButton)
    return SetBinding(key, "CLICK " .. buttonName .. ":" .. (mouseButton or "LeftButton"))
end
function SetBindingSpell(key, spell) return SetBinding(key, "SPELL " .. spell) end
function SetBindingMacro(key, macro) return SetBinding(key, "MACRO " .. tostring(macro)) end
function SetBindingItem(key, item) return SetBinding(key, "ITEM " .. item) end
-- The bindings that differ from the defaults, in the client's
-- bindings-cache.wtf format ("bind KEY COMMAND", NONE for unbound).
local function serialize()
    local keys, seen = {}, {}
    for k in pairs(bindings) do if not seen[k] then seen[k] = true; keys[#keys + 1] = k end end
    for k in pairs(DEFAULTS) do if not seen[k] then seen[k] = true; keys[#keys + 1] = k end end
    table.sort(keys)
    local lines = {}
    for _, k in ipairs(keys) do
        if bindings[k] ~= DEFAULTS[k] then
            lines[#lines + 1] = "bind " .. k .. " " .. (bindings[k] or "NONE")
        end
    end
    return table.concat(lines, "\n") .. (#lines > 0 and "\n" or "")
end

-- Apply a bindings-cache.wtf file over the current bindings.
local function apply(text)
    for line in (text .. "\n"):gmatch("([^\r\n]*)\r?\n") do
        local key, command = line:match("^bind%s+(%S+)%s+(.-)%s*$")
        key = normalize(key)
        if key then bindings[key] = command ~= "NONE" and command or nil end
    end
end

function SaveBindings() saveFile(serialize()) end
function LoadBindings() bindingsChanged() end
function GetCurrentBindingSet() return 2 end -- Enum.BindingSet.Character

local function setOverride(owner, priority, key, command)
    key = normalize(key)
    if not key then return end
    for i = #overrides, 1, -1 do
        if overrides[i].owner == owner and overrides[i].key == key then table.remove(overrides, i) end
    end
    if command and command ~= "" then
        overrides[#overrides + 1] = { owner = owner, priority = priority and true or false,
                                      key = key, command = command }
    end
    bindingsChanged()
end

function SetOverrideBinding(owner, priority, key, command)
    setOverride(owner, priority, key, command)
end
function SetOverrideBindingClick(owner, priority, key, buttonName, mouseButton)
    local command = buttonName and ("CLICK " .. buttonName .. ":" .. (mouseButton or "LeftButton"))
    setOverride(owner, priority, key, command)
end
function SetOverrideBindingSpell(owner, priority, key, spell)
    setOverride(owner, priority, key, spell and ("SPELL " .. spell))
end
function SetOverrideBindingMacro(owner, priority, key, macro)
    setOverride(owner, priority, key, macro and ("MACRO " .. tostring(macro)))
end
function SetOverrideBindingItem(owner, priority, key, item)
    setOverride(owner, priority, key, item and ("ITEM " .. item))
end
function ClearOverrideBindings(owner)
    for i = #overrides, 1, -1 do
        if overrides[i].owner == owner then table.remove(overrides, i) end
    end
    bindingsChanged()
end

return run, apply
