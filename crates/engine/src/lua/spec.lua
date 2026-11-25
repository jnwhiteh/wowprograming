-- Interaction specs: Lua files that drive the sandbox like a player and
-- check the result (`wowprogramming test`). A spec runs as a coroutine;
-- Wait, Hold, Command and Reload hand control back to the engine, which
-- advances game time or runs the command, then resumes the spec.
--
-- Helpers (also as Sandbox.Name):
--   Wait(seconds)            let game time pass
--   Click(frame [, button])  move the mouse to a frame and click it
--   ClickAt(x, y [, button]) click at a screen position (UI units from
--                            the bottom left), such as a unit in the world
--   Hover(frame)             move the mouse to a frame
--   Press(key)               press and release a key ("W", "SHIFT-P")
--   Hold(key, seconds)       hold a key while game time passes
--   KeyDown(key), KeyUp(key)
--   Type(text)               type into the focused edit box
--   Command(line)            a console line ("/way 42 60", "/reload")
--   Reload()                 reload the UI
--   Equal(actual, expected [, message]), Near(actual, expected, tolerance [, message])
-- A frame is a widget, a global name, or an expression such as
-- "PartyFrame.MemberFrame1".
local natives = ...
local type, tostring, error, setmetatable, setfenv, loadstring, coroutine, _G =
      type, tostring, error, setmetatable, setfenv, loadstring, coroutine, _G

local T = {}

local function frameOf(f)
    if type(f) == "table" then return f end
    if type(f) == "string" then
        local fn = loadstring("return " .. f)
        local ok, value = pcall(fn)
        if ok and type(value) == "table" then return value end
    end
    error("not a frame: " .. tostring(f), 3)
end

function T.Wait(seconds) coroutine.yield("wait", seconds or 0) end
function T.Command(line) coroutine.yield("command", line) end
function T.Reload() coroutine.yield("reload") end

function T.Hover(f)
    local frame = frameOf(f)
    local x, y = frame:GetCenter()
    if not x then error("frame has no position: " .. tostring(f), 2) end
    natives.move(x, y)
end

function T.Click(f, button)
    T.Hover(f)
    natives.down(button or "LeftButton")
    natives.up(button or "LeftButton")
end

function T.ClickAt(x, y, button)
    natives.move(x, y)
    natives.down(button or "LeftButton")
    natives.up(button or "LeftButton")
end

function T.KeyDown(key) return natives.keydown(key) end
function T.KeyUp(key) natives.keyup(key) end

function T.Press(key)
    if not natives.keydown(key) then error("nothing took the key " .. key, 2) end
    natives.keyup(key)
end

function T.Hold(key, seconds)
    if not natives.keydown(key) then error("nothing took the key " .. key, 2) end
    T.Wait(seconds)
    natives.keyup(key)
end

function T.Type(text)
    if not natives.type(text) then error("no edit box has the focus", 2) end
end

function T.Equal(actual, expected, message)
    if actual ~= expected then
        error((message and (message .. ": ") or "") .. "expected " .. tostring(expected)
            .. ", got " .. tostring(actual), 2)
    end
end

function T.Near(actual, expected, tolerance, message)
    if type(actual) ~= "number" or math.abs(actual - expected) > (tolerance or 0.01) then
        error((message and (message .. ": ") or "") .. "expected " .. tostring(expected)
            .. " +- " .. tostring(tolerance) .. ", got " .. tostring(actual), 2)
    end
end

_G.Sandbox = T

-- Compile a spec and return its coroutine. Globals the spec sets stay in
-- its own environment; reads fall through to the helpers, then _G.
return function(code, name)
    local chunk, err = loadstring(code, "@" .. name)
    if not chunk then error(err, 0) end
    local env = setmetatable({}, { __index = function(_, k)
        local v = T[k]
        if v ~= nil then return v end
        return _G[k]
    end })
    setfenv(chunk, env)
    return coroutine.create(function()
        chunk()
        return "done"
    end)
end
