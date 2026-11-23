-- WowProgramming event trace (addon !WowProgrammingTrace)
--
-- Records, without any command, the events the client sends while a
-- character logs in, reloads or changes zone, so the sandbox can send the
-- same events in the same order. It also records a short profile of the
-- character (level, zone, equipment) for the sandbox's character select
-- screen.
--
-- Recording starts when this file loads and stops 90 seconds after the
-- first PLAYER_ENTERING_WORLD. It starts again at each LOADING_SCREEN_ENABLED
-- and stops 30 seconds after the next LOADING_SCREEN_DISABLED.
--
-- Each event is one string: "<ms since load>\t<event>\t<arg1>\t<arg2>...".
-- Chat and combat log events keep only their name (no message text).
-- The game writes the data at logout or /reload, into
-- WTF/Account/<account>/SavedVariables/!WowProgrammingTrace.lua.
-- The "!" in the addon name makes it load before most other addons. The last 4 sessions per character are kept.

local ADDON = ...
local MAX_EVENTS = 60000
local MAX_SESSIONS = 4
local AFTER_LOGIN = 90
local AFTER_LOADING = 30

local started = debugprofilestop()
local events = {}
local recording = true
local stopToken = 0
local session = {
	events = events,
	startTime = time(),
	build = select(4, GetBuildInfo()),
}

-- Addons already loaded when this file runs: shows where in the load order
-- the trace starts.
do
	local before = {}
	if C_AddOns and C_AddOns.GetNumAddOns then
		for i = 1, C_AddOns.GetNumAddOns() do
			local name = C_AddOns.GetAddOnInfo(i)
			if name and C_AddOns.IsAddOnLoaded(i) then
				before[#before + 1] = name
			end
		end
	end
	session.loadedBefore = before
end

local function isSecret(v)
	return issecretvalue and issecretvalue(v)
end

local function text(v)
	if isSecret(v) then return "<secret>" end
	local t = type(v)
	if t == "string" then
		v = v:gsub("[\t\n\r]", " ")
		if #v > 120 then v = v:sub(1, 120) .. "..." end
		return v
	elseif t == "number" or t == "boolean" or t == "nil" then
		return tostring(v)
	end
	return "<" .. t .. ">"
end

local function nameOnly(event)
	return event:find("^CHAT_MSG_") or event == "COMBAT_LOG_EVENT_UNFILTERED"
		or event == "COMBAT_LOG_EVENT"
end

local function stopAfter(seconds)
	stopToken = stopToken + 1
	local token = stopToken
	C_Timer.After(seconds, function()
		if token == stopToken then
			recording = false
			events[#events + 1] = string.format("%d\t<stop>", debugprofilestop() - started)
		end
	end)
end

-- Calls a client function by name; a missing or failing one gives nil.
local function try(name, ...)
	local f = _G
	for part in name:gmatch("[^.]+") do
		f = type(f) == "table" and f[part] or nil
	end
	if type(f) ~= "function" then return nil end
	local r = { pcall(f, ...) }
	if not r[1] then return nil end
	return select(2, unpack(r))
end

local function profile()
	local p = {
		name = try("UnitName", "player"),
		realm = try("GetRealmName"),
		guid = try("UnitGUID", "player"),
		level = try("UnitLevel", "player"),
		raceName = try("UnitRace", "player"),
		race = select(2, try("UnitRace", "player")),
		class = select(2, try("UnitClass", "player")),
		sex = try("UnitSex", "player"),
		faction = try("UnitFactionGroup", "player"),
		zone = try("GetZoneText"),
		realZone = try("GetRealZoneText"),
		subZone = try("GetSubZoneText"),
		mapID = try("C_Map.GetBestMapForUnit", "player"),
		guild = try("GetGuildInfo", "player"),
		time = time(),
		items = {},
	}
	local _, instanceType, _, _, _, _, _, instanceID = try("GetInstanceInfo")
	p.instanceType, p.instanceID = instanceType, instanceID
	p.itemLevel, p.itemLevelEquipped = try("GetAverageItemLevel")
	p.specIndex = try("GetSpecialization")
	if p.specIndex then p.specID = try("GetSpecializationInfo", p.specIndex) end
	for slot = 1, 19 do
		local link = try("GetInventoryItemLink", "player", slot)
		if link and not isSecret(link) then p.items[slot] = link end
	end
	for k, v in pairs(p) do
		if isSecret(v) then p[k] = "<secret>" end
	end
	return p
end

local frame = CreateFrame("Frame")
frame:RegisterAllEvents()
frame:SetScript("OnEvent", function(_, event, ...)
	if event == "LOADING_SCREEN_ENABLED" and not recording then
		recording = true
		events[#events + 1] = string.format("%d\t<start>", debugprofilestop() - started)
	end
	if recording and #events < MAX_EVENTS then
		local line = string.format("%d\t%s", debugprofilestop() - started, event)
		if not nameOnly(event) then
			local n = select("#", ...)
			if n > 0 then
				local parts = {}
				for i = 1, n do
					local ok, s = pcall(text, (select(i, ...)))
					parts[i] = ok and s or "<error>"
				end
				line = line .. "\t" .. table.concat(parts, "\t")
			end
		end
		events[#events + 1] = line
	end
	if event == "ADDON_LOADED" and ... == ADDON then
		WowProgrammingTraceDB = WowProgrammingTraceDB or {}
	elseif event == "PLAYER_ENTERING_WORLD" then
		local isLogin, isReload = ...
		if not session.kind then
			session.kind = isLogin and "login" or isReload and "reload" or "other"
			stopAfter(AFTER_LOGIN)
		end
		-- Profile after the zone text and item level settle.
		C_Timer.After(5, function()
			local ok, p = pcall(profile)
			if ok then
				session.profile = p
			else
				session.profileError = tostring(p)
			end
		end)
	elseif event == "LOADING_SCREEN_DISABLED" and session.kind then
		stopAfter(AFTER_LOADING)
	elseif event == "PLAYER_LOGOUT" then
		session.endTime = time()
		session.truncated = #events >= MAX_EVENTS
		local key = (UnitName("player") or "?") .. "-" .. (GetRealmName() or "?")
		WowProgrammingTraceDB = WowProgrammingTraceDB or {}
		local list = WowProgrammingTraceDB[key] or {}
		list[#list + 1] = session
		while #list > MAX_SESSIONS do table.remove(list, 1) end
		WowProgrammingTraceDB[key] = list
	end
end)
