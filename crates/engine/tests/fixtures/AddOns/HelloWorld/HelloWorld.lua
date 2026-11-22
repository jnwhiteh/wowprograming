local addonName, ns = ...
ns.greeting = "Hello from " .. addonName

local frame = CreateFrame("Frame", "HelloWorldFrame")
frame:RegisterEvent("ADDON_LOADED")
frame:RegisterEvent("PLAYER_LOGIN")
frame:SetScript("OnEvent", function(self, event, arg1)
    if event == "ADDON_LOADED" and arg1 == addonName then
        HelloWorldDB = HelloWorldDB or { logins = 0 }
    elseif event == "PLAYER_LOGIN" then
        HelloWorldDB.logins = HelloWorldDB.logins + 1
        print(ns.greeting .. ", login #" .. HelloWorldDB.logins)
    end
end)

SLASH_HELLOWORLD1 = "/hello"
SLASH_HELLOWORLD2 = "/hw"
SlashCmdList.HELLOWORLD = function(msg)
    print("You said: " .. msg)
end
