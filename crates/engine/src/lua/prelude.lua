local defaultErrorHandler, defaultPrint, GetCallstackHeight, normalizeError = ...
local type, select, unpack, error, tostring, xpcall, pairs, loadstring, _G =
      type, select, unpack, error, tostring, xpcall, pairs, loadstring, _G

local errorHandler = defaultErrorHandler
function seterrorhandler(handler)
    if type(handler) ~= "function" then error("Usage: seterrorhandler(errfunc)", 2) end
    errorHandler = handler
end
function geterrorhandler() return errorHandler end

-- Height of the stack at the point of the last error, counted from the
-- frame that raised it. Blizzard's error handler uses it to find the
-- stack level for debugstack.
local errorCallstackHeight = nil
function GetErrorCallstackHeight() return errorCallstackHeight end
function SetErrorCallstackHeight(height) errorCallstackHeight = height end
local function callErrorHandler(msg)
    errorCallstackHeight = GetCallstackHeight() - 1
    return errorHandler(normalizeError(msg))
end

local printHandler = defaultPrint
function setprinthandler(handler)
    if type(handler) ~= "function" then error("Usage: setprinthandler(func)", 2) end
    printHandler = handler
end
function getprinthandler() return printHandler end
function print(...) return printHandler(...) end

-- Call f with the arguments; errors go to the error handler.
-- Returns true and the results, or false. It lives in its own chunk so
-- that stack traces can stop at this boundary between engine and Lua.
local dispatch = loadstring([[
local xpcall, select, unpack, handler = ...
return function(f, ...)
    local n = select("#", ...)
    if n == 0 then return xpcall(f, handler) end
    local args = { ... }
    return xpcall(function() return f(unpack(args, 1, n)) end, handler)
end
]], "=[dispatch]")(xpcall, select, unpack, callErrorHandler)

local function results(ok, ...) if ok then return ... end end

function securecall(f, ...)
    if type(f) == "string" then f = _G[f] end
    if type(f) ~= "function" then error("securecall(): function expected", 2) end
    return results(dispatch(f, ...))
end
function securecallfunction(f, ...) return results(dispatch(f, ...)) end
function securecallmethod(object, method, ...)
    return results(dispatch(object[method], object, ...))
end
function secureexecuterange(tbl, f, ...)
    for k, v in pairs(tbl) do dispatch(f, k, v, ...) end
end

function hooksecurefunc(tbl, name, hook)
    if type(tbl) ~= "table" then tbl, name, hook = _G, tbl, name end
    local original = tbl[name]
    if type(original) ~= "function" then
        error("hooksecurefunc(): " .. tostring(name) .. " is not a function", 2)
    end
    if type(hook) ~= "function" then
        error('Usage: hooksecurefunc([table,] "functionName", hookfunc)', 2)
    end
    tbl[name] = function(...)
        local n = select("#", ...)
        local args = { ... }
        return (function(...)
            dispatch(hook, unpack(args, 1, n))
            return ...
        end)(original(...))
    end
end

-- Taint and secret values are not emulated yet. All code counts as secure.
function issecure() return true end
-- Interim rule until taint is tracked: a key in a table other than _G,
-- asked about from a third-party addon's code, is tainted by that addon.
-- TaintLess uses this to learn its own addon name. Globals and Blizzard
-- code are secure.
function issecurevariable(tbl, key)
    if type(tbl) ~= "table" or tbl == _G then return true, nil end
    local stack = debugstack and debugstack(2, 1, 0) or ""
    local addon = stack:match("AddOns[/\\]([^/\\]+)[/\\]")
    if addon and not addon:match("^Blizzard_") then return false, addon end
    return true, nil
end
function forceinsecure() end
function issecretvalue() return false end
function canaccessvalue() return true end
function CreateSecureDelegate(f) return f end
function scrub(...)
    local n = select("#", ...)
    local t = { ... }
    for i = 1, n do
        local ty = type(t[i])
        if ty ~= "string" and ty ~= "number" and ty ~= "boolean" then t[i] = nil end
    end
    return unpack(t, 1, n)
end
function debuglocals() return "" end
function GetCurrentEnvironment() return _G end
-- Taint is not tracked yet: compiling "untainted" is plain compiling.
loadstring_untainted = loadstring
-- fastrandom: the client's quicker random, with math.random's arguments.
fastrandom = math.random

return dispatch, callErrorHandler
