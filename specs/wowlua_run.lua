-- WowLua: open the window, type code, run it, keep it across /reload.
-- wowprogramming test --scenario scenarios/dummy --addon WowLua specs/wowlua_run.lua
Command("/wowlua")
Wait(0.2)
Equal(WowLuaFrame:IsShown(), true, "window shown")
Click("WowLuaFrameEditBox")
Type("print('hello', 6 * 7)")
Click("WowLuaButton_Run")
Wait(0.1)
local out = WowLuaFrameOutput:GetMessageInfo(WowLuaFrameOutput:GetNumMessages())
assert(out:find("hello, 42"), "output was " .. tostring(out))

Click("WowLuaButton_Save")
Reload()
local _, page = WowLua:GetCurrentPage()
Equal(page.content, "print('hello', 6 * 7)", "saved page")
