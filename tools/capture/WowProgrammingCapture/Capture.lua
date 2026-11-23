-- WowProgramming Capture
--
-- Records what the sandbox cannot derive from the game files or the UI
-- source, for one character:
--
--   calls     every client API call the default UI makes while its frames
--             refresh, with arguments and results (replayed by scenarios)
--   frames    position and art of shown parts of the spellbook, unit frames,
--             party and raid frames and action bars (for layout comparison)
--   atlases   C_Texture.GetAtlasInfo for every atlas seen in those frames
--   constants named colors, Enum, Constants and global numbers
--   enums     enums that do not show up when walking Enum
--   cvars     every console variable with its value and default
--   fonts     measured text widths and heights for the common fonts
--   editMode  edit mode layouts (where the HUD frames are placed)
--   meta      build, screen size, UI scale, class colors and names
--
-- Use: target something (a training dummy is ideal), join a party if you
-- want party frames, then type /wpcapture out of combat. It opens the
-- spellbook for a moment. When it prints "done", type /reload: that writes
-- the data to WTF/Account/<account>/SavedVariables/WowProgrammingCapture.lua.
-- The /reload also clears the taint that the recording leaves behind.

local ADDON = ...
local VERSION = 2

WowProgrammingCaptureDB = WowProgrammingCaptureDB or {}

local pack = table.pack or function(...) return { n = select("#", ...), ... } end

local function argKey(...)
	local parts = {}
	for i = 1, select("#", ...) do
		local v = select(i, ...)
		if issecretvalue and issecretvalue(v) then
			parts[i] = "<secret>"
		else
			parts[i] = tostring(v)
		end
	end
	return table.concat(parts, ",")
end

local function isSecret(v)
	return issecretvalue and issecretvalue(v)
end

-- Plain values only: the serializer rejects functions and userdata, and
-- secret values must not be read.
local function plain(v, seen, depth)
	depth = depth or 0
	if isSecret(v) then
		return "<secret>"
	end
	local t = type(v)
	if t == "string" or t == "number" or t == "boolean" then
		return v
	elseif t == "table" then
		if seen[v] or depth > 12 then return nil end
		seen[v] = true
		local out = {}
		for k, val in pairs(v) do
			local kt = type(k)
			if (kt == "string" or kt == "number") and not isSecret(k) then
				local pv = plain(val, seen, depth + 1)
				if pv ~= nil then out[k] = pv end
			end
		end
		seen[v] = nil
		return out
	end
	return nil
end

local data, calls, errors

-- Recording ------------------------------------------------------------------

local MAX_ARGS_PER_FUNCTION = 600

-- Functions not worth recording: clocks, frame creation, hooks, logging.
local SKIP = {
	GetTime = true, GetTimePreciseSec = true, GetServerTime = true, GetSessionTime = true,
	debugprofilestop = true, debugprofilestart = true, GetFramerate = true,
	CreateFrame = true, hooksecurefunc = true, securecall = true, securecallfunction = true,
	secureexecuterange = true, issecure = true, issecurevariable = true, forceinsecure = true,
	GetCursorPosition = true, GetMouseFoci = true, GetMouseFocus = true, IsMouseButtonDown = true,
	GetCallstackHeight = true, GetErrorCallstackHeight = true, geterrorhandler = true,
	seterrorhandler = true, getprinthandler = true, setprinthandler = true, print = true,
	CopyToClipboard = true, PlaySound = true, PlaySoundFile = true, StopSound = true,
	RunScript = true, InCombatLockdown = true, GetCVar = true, GetCVarBool = true,
	GetCVarDefault = true, SetCVar = true, GetText = true, issecretvalue = true,
	canaccessvalue = true, canaccesssecrets = true, canaccesstable = true,
}
local SKIP_NAMESPACES = {
	C_Timer = true, C_CVar = true, C_Log = true, C_EncodingUtil = true,
	C_StringUtil = true, C_FunctionContainers = true, C_Debug = true,
	C_CurveUtil = true, C_DurationUtil = true,
}

-- Global functions worth wrapping: client API names have no underscore
-- (Lua helpers in FrameXML usually do).
local GLOBAL_PREFIXES = {
	"Unit", "Get", "Is", "Has", "Can", "In", "Num", "Should", "Are", "Does",
	"BN", "Guild", "Party", "Raid", "Pet", "Spell", "Action",
}

local function wantGlobal(name)
	if SKIP[name] or name:find("_") then return false end
	for _, p in ipairs(GLOBAL_PREFIXES) do
		if name:sub(1, #p) == p then return true end
	end
	return false
end

local function store(name, key, r)
	local byArgs = calls[name]
	if not byArgs then
		byArgs = { ["#"] = 0 }
		calls[name] = byArgs
	end
	if byArgs[key] == nil then
		if byArgs["#"] >= MAX_ARGS_PER_FUNCTION then return end
		byArgs["#"] = byArgs["#"] + 1
	end
	local out = { n = r.n }
	for i = 1, r.n do
		out[tostring(i)] = plain(r[i], {})
	end
	byArgs[key] = out
end

local originals = {}

local function wrap(container, key, fullName)
	local original = container[key]
	local function recorder(...)
		local r = pack(original(...))
		local ok, err = pcall(store, fullName, argKey(...), r)
		if not ok and #errors < 200 then
			table.insert(errors, "record " .. fullName .. ": " .. tostring(err))
		end
		return unpack(r, 1, r.n)
	end
	local ok = pcall(function() container[key] = recorder end)
	if ok then
		table.insert(originals, { container, key, original })
	end
end

local function startRecording()
	for name, value in pairs(_G) do
		if type(name) == "string" then
			if type(value) == "function" and wantGlobal(name) then
				wrap(_G, name, name)
			elseif type(value) == "table" and name:match("^C_") and not SKIP_NAMESPACES[name] then
				for key, fn in pairs(value) do
					if type(key) == "string" and type(fn) == "function" then
						wrap(value, key, name .. "." .. key)
					end
				end
			end
		end
	end
end

local function stopRecording()
	for i = #originals, 1, -1 do
		local o = originals[i]
		pcall(function() o[1][o[2]] = o[3] end)
	end
	wipe(originals)
end

-- Calls a function and records it even when the UI does not call it.
local function rec(name, ...)
	local container, key = _G, name
	for part, rest in name:gmatch("([^.]+)%.(.+)") do
		container, key = container[part], rest
	end
	local fn = type(container) == "table" and container[key]
	if type(fn) ~= "function" then
		table.insert(errors, "missing function " .. name)
		return
	end
	local r = pack(pcall(fn, ...))
	if not r[1] then
		table.insert(errors, name .. "(" .. argKey(...) .. "): " .. tostring(r[2]))
		return
	end
	local results = { n = r.n - 1 }
	for i = 2, r.n do results[i - 1] = r[i] end
	store(name, argKey(...), results)
	return unpack(results, 1, results.n)
end

-- Refreshing the UI --------------------------------------------------------

local UNITS = { "player", "target", "targettarget", "focus", "pet", "vehicle",
	"party1", "party2", "party3", "party4", "mouseover" }
local UNIT_EVENTS = {
	"UNIT_NAME_UPDATE", "UNIT_LEVEL", "UNIT_HEALTH", "UNIT_MAXHEALTH", "UNIT_POWER_UPDATE",
	"UNIT_MAXPOWER", "UNIT_DISPLAYPOWER", "UNIT_FACTION", "UNIT_FLAGS", "UNIT_CLASSIFICATION_CHANGED",
	"UNIT_PORTRAIT_UPDATE", "UNIT_MODEL_CHANGED", "UNIT_CONNECTION", "UNIT_PHASE", "UNIT_PET",
	"UNIT_THREAT_SITUATION_UPDATE", "UNIT_ABSORB_AMOUNT_CHANGED", "UNIT_HEAL_ABSORB_AMOUNT_CHANGED",
	"UNIT_HEAL_PREDICTION", "UNIT_TARGET", "UNIT_OTHER_PARTY_CHANGED", "UNIT_POWER_BAR_SHOW",
}
local PLAIN_EVENTS = {
	"PLAYER_TARGET_CHANGED", "PLAYER_FOCUS_CHANGED", "GROUP_ROSTER_UPDATE", "PARTY_LEADER_CHANGED",
	"RAID_TARGET_UPDATE", "PLAYER_FLAGS_CHANGED", "PLAYER_UPDATE_RESTING", "PVP_TIMER_UPDATE",
	"UPDATE_BINDINGS", "SPELLS_CHANGED", "UPDATE_SHAPESHIFT_FORM", "UPDATE_SHAPESHIFT_FORMS",
	"ACTIONBAR_PAGE_CHANGED", "ACTIONBAR_UPDATE_STATE", "ACTIONBAR_UPDATE_USABLE",
	"ACTIONBAR_UPDATE_COOLDOWN", "SPELL_UPDATE_COOLDOWN", "SPELL_UPDATE_USABLE", "PET_BAR_UPDATE",
	"UPDATE_VEHICLE_ACTIONBAR", "UPDATE_OVERRIDE_ACTIONBAR", "UPDATE_POSSESS_BAR",
	"PLAYER_REGEN_ENABLED", "TRAIT_CONFIG_UPDATED", "ACTIVE_TALENT_GROUP_CHANGED",
}

local function eventCalls()
	local list = {}
	table.insert(list, { "PLAYER_ENTERING_WORLD", false, true })
	table.insert(list, { "ACTIONBAR_SLOT_CHANGED", 0 })
	table.insert(list, { "PLAYER_SPECIALIZATION_CHANGED", "player" })
	for _, e in ipairs(PLAIN_EVENTS) do
		table.insert(list, { e })
	end
	for _, unit in ipairs(UNITS) do
		table.insert(list, { "UNIT_AURA", unit, { isFullUpdate = true } })
		for _, e in ipairs(UNIT_EVENTS) do
			table.insert(list, { e, unit })
		end
	end
	return list
end

-- Call the OnEvent handler of every frame registered for each event.
-- EventRegistry's frame is skipped: its callbacks include protected setup
-- (the macro execution callback on PLAYER_ENTERING_WORLD) and systems that
-- refuse tainted callers (the cooldown viewer).
local function refreshFrames()
	local list = eventCalls()
	local skip = EventRegistry and EventRegistry.frameEventFrame
	local frame = EnumerateFrames()
	local handled = 0
	while frame do
		local okForbidden, forbidden = pcall(frame.IsForbidden, frame)
		if okForbidden and not forbidden and frame ~= skip then
			local okScript, onEvent = pcall(frame.GetScript, frame, "OnEvent")
			if okScript and onEvent then
				for _, call in ipairs(list) do
					local okReg, registered = pcall(frame.IsEventRegistered, frame, call[1])
					if okReg and registered then
						local ok, err = pcall(onEvent, frame, unpack(call))
						handled = handled + 1
						if not ok and #errors < 200 then
							table.insert(errors, "event " .. call[1] .. ": " .. tostring(err))
						end
					end
				end
			end
		end
		frame = EnumerateFrames(frame)
	end
	return handled
end

-- Frames -------------------------------------------------------------------

local function round(v)
	if type(v) ~= "number" or isSecret(v) then return nil end
	return math.floor(v * 100 + 0.5) / 100
end

local function recordObject(list, atlases, obj, depth)
	if #list >= 6000 then return end
	local okShown, shown = pcall(obj.IsShown, obj)
	if not okShown or not shown then return end
	local entry = {
		path = obj.GetDebugName and obj:GetDebugName() or "?",
		type = obj:GetObjectType(),
		depth = depth,
		alpha = round(obj:GetAlpha()),
	}
	local left, bottom, width, height = obj:GetRect()
	if left and not isSecret(left) then
		entry.rect = { round(left), round(bottom), round(width), round(height) }
	end
	if obj.GetFrameLevel then
		entry.level = obj:GetFrameLevel()
		entry.strata = obj:GetFrameStrata()
		entry.scale = round(obj:GetScale())
	end
	if obj.GetDrawLayer then
		local layer, sublevel = obj:GetDrawLayer()
		entry.layer, entry.sublevel = layer, sublevel
	end
	if entry.type == "Texture" or entry.type == "MaskTexture" then
		local atlas = obj:GetAtlas()
		if atlas then
			entry.atlas = atlas
			if atlases[atlas] == nil then
				atlases[atlas] = C_Texture.GetAtlasInfo(atlas) or false
			end
		end
		entry.texture = obj:GetTexture()
		local coords = { obj:GetTexCoord() }
		for i, v in ipairs(coords) do coords[i] = round(v) end
		entry.texCoord = coords
		entry.vertexColor = { obj:GetVertexColor() }
		entry.blend = obj:GetBlendMode()
		entry.desaturated = obj:IsDesaturated()
		if obj.GetNumMaskTextures then entry.masks = obj:GetNumMaskTextures() end
	elseif entry.type == "FontString" then
		local text = obj:GetText()
		entry.text = not isSecret(text) and text or "<secret>"
		local font, size, flags = obj:GetFont()
		entry.font, entry.fontSize, entry.fontFlags = font, round(size), flags
		entry.textColor = { obj:GetTextColor() }
		entry.stringWidth = round(obj:GetStringWidth())
		entry.stringHeight = round(obj:GetStringHeight())
		entry.justifyH, entry.justifyV = obj:GetJustifyH(), obj:GetJustifyV()
	elseif entry.type == "StatusBar" then
		local minValue, maxValue = obj:GetMinMaxValues()
		entry.minMax = { round(minValue), round(maxValue) }
		entry.value = round(obj:GetValue())
	end
	table.insert(list, entry)
	if obj.GetRegions then
		for _, region in ipairs({ obj:GetRegions() }) do
			recordObject(list, atlases, region, depth + 1)
		end
	end
	if obj.GetChildren and depth < 20 then
		for _, child in ipairs({ obj:GetChildren() }) do
			recordObject(list, atlases, child, depth + 1)
		end
	end
end

local FRAME_TREES = {
	"PlayerFrame", "TargetFrame", "TargetFrameToT", "FocusFrame", "PetFrame", "PartyFrame",
	"CompactPartyFrame", "CompactRaidFrameContainer", "CompactRaidFrameManager",
	"MainActionBar", "MainMenuBar", "MultiBarBottomLeft", "MultiBarBottomRight", "MultiBarRight",
	"MultiBarLeft", "MultiBar5", "MultiBar6", "MultiBar7", "StanceBar", "PetActionBar",
	"MicroMenuContainer", "BagsBar", "BuffFrame", "DebuffFrame", "PlayerCastingBarFrame",
	"StatusTrackingBarManager", "MinimapCluster", "ObjectiveTrackerFrame", "ChatFrame1",
	"PlayerSpellsFrame",
}

-- Fonts --------------------------------------------------------------------

local FONT_FILES = {
	"Fonts\\FRIZQT__.TTF", "Fonts\\ARIALN.TTF", "Fonts\\MORPHEUS.TTF", "Fonts\\skurri.ttf",
}
local FONT_SIZES = { 8, 9, 10, 11, 12, 13, 14, 15, 16, 18, 20, 22, 24, 28, 32 }
local SAMPLES = { "Page 1/5", "Spellbook", "Blackout Kick", "Crackling Jade Lightning",
	"The quick brown fox jumps over the lazy dog", "12,345 / 67,890", "100%" }

local function measureFonts()
	local fonts = {}
	local fs = UIParent:CreateFontString(nil, "OVERLAY")
	fs:SetWordWrap(false)
	for _, path in ipairs(FONT_FILES) do
		for _, size in ipairs(FONT_SIZES) do
			local okFont, set = pcall(fs.SetFont, fs, path, size, "")
			if okFont and set then
				local entry = { advance = {}, repeated = {}, samples = {} }
				fs:SetText("A")
				entry.height = round(fs:GetStringHeight())
				entry.lineHeight = fs.GetLineHeight and round(fs:GetLineHeight()) or nil
				for code = 32, 126 do
					local c = string.char(code)
					fs:SetText(c)
					entry.advance[tostring(code)] = round(fs:GetUnboundedStringWidth())
					fs:SetText(c:rep(10))
					entry.repeated[tostring(code)] = round(fs:GetUnboundedStringWidth())
				end
				for _, s in ipairs(SAMPLES) do
					fs:SetText(s)
					entry.samples[s] = round(fs:GetUnboundedStringWidth())
				end
				fonts[path .. "|" .. size] = entry
			end
		end
	end
	fs:Hide()
	return fonts
end

-- Constants, enums and CVars ----------------------------------------------

local function copyTable(t, depth)
	local out = {}
	for k, v in pairs(t) do
		local kt, vt = type(k), type(v)
		if kt == "string" or kt == "number" then
			if vt == "table" and depth < 3 then
				out[tostring(k)] = copyTable(v, depth + 1)
			elseif vt == "number" or vt == "string" or vt == "boolean" then
				out[tostring(k)] = v
			end
		end
	end
	return out
end

-- Enums that the UI uses but that were missing when walking Enum.
local NAMED_ENUMS = {
	"StoreError", "VasServiceType", "VasPurchaseState", "VasQueueStatus", "PurchaseEligibility",
	"BattlepayBannerType", "BattlepayBoostProduct", "BattlepayCardType", "BattlepayDisplayFlag",
	"BattlepayGroupDisplayType", "BattlepayProductDecorator", "BattlepayProductGroupFlag",
	"ValueAddedServiceType", "GuideFrameState", "CharacterCreateType",
}

local function captureConstants()
	local colors = {}
	for _, c in ipairs(C_UIColor.GetColors()) do
		table.insert(colors, { baseTag = c.baseTag, r = c.color.r, g = c.color.g, b = c.color.b, a = c.color.a })
	end
	local numbers = {}
	for k, v in pairs(_G) do
		local vt = type(v)
		if type(k) == "string" and (vt == "number" or vt == "boolean") then
			numbers[k] = v
		end
	end
	local enums = {}
	for _, name in ipairs(NAMED_ENUMS) do
		local e = Enum[name]
		if type(e) == "table" then
			enums[name] = copyTable(e, 2)
		end
	end
	return {
		colors = colors,
		enum = copyTable(Enum, 1),
		constants = copyTable(Constants, 1),
		numbers = numbers,
	}, enums
end

local function captureCVars()
	local cvars = {}
	if not ConsoleGetAllCommands then return cvars end
	local cvarType = Enum.ConsoleCommandType and Enum.ConsoleCommandType.Cvar
	for _, cmd in ipairs(ConsoleGetAllCommands()) do
		if cmd.commandType == cvarType then
			local value, default, account, character, locked, secure, readonly = C_CVar.GetCVarInfo(cmd.command)
			if value ~= nil then
				cvars[cmd.command] = { value = value, default = default, account = account,
					character = character, readonly = readonly }
			end
		end
	end
	return cvars
end

-- Explicit calls -------------------------------------------------------------

-- Calls the UI makes only in situations the refresh does not reach.
local function explicitCalls()
	local _, _, classID = UnitClass("player")
	rec("C_AssistedCombat.IsAvailable")
	rec("C_AssistedCombat.GetActionSpell")
	rec("C_AssistedCombat.GetRotationSpells")
	rec("C_SpecializationInfo.CanPlayerUseTalentUI")
	rec("C_SpecializationInfo.CanPlayerUseTalentSpecUI")
	rec("IsPlayerInitialSpec")
	rec("GetSpecializationRoleEnum", 1)
	local numSpecs = GetNumSpecializations() or 0
	for i = 1, numSpecs do
		local specID = GetSpecializationInfo(i)
		rec("GetSpecializationInfo", i)
		rec("GetSpecializationRoleEnum", i)
		if specID then
			rec("C_SpecializationInfo.GetSpellsDisplay", specID)
			rec("GetSpecializationInfoByID", specID)
		end
	end
	rec("C_ClassColor.GetClassColor", select(2, UnitClass("player")))
	-- Spellbook items: info, tooltips and descriptions.
	local seenSpells = {}
	local banks = { Enum.SpellBookSpellBank.Player, Enum.SpellBookSpellBank.Pet }
	for _, bank in ipairs(banks) do
		local numLines = bank == Enum.SpellBookSpellBank.Player and C_SpellBook.GetNumSpellBookSkillLines() or 1
		for line = 1, numLines do
			local info = C_SpellBook.GetSpellBookSkillLineInfo(line)
			local first, count = 0, 0
			if bank == Enum.SpellBookSpellBank.Player and info then
				first, count = info.itemIndexOffset or 0, info.numSpellBookItems or 0
			elseif bank == Enum.SpellBookSpellBank.Pet then
				first, count = 0, (C_SpellBook.HasPetSpells() or 0)
			end
			for slot = first + 1, first + count do
				local item = rec("C_SpellBook.GetSpellBookItemInfo", slot, bank)
				rec("C_TooltipInfo.GetSpellBookItem", slot, bank)
				if item and item.spellID and not seenSpells[item.spellID] then
					seenSpells[item.spellID] = true
					rec("C_TooltipInfo.GetSpellByID", item.spellID)
					rec("C_Spell.GetSpellDescription", item.spellID)
				end
				if item and item.itemType == Enum.SpellBookItemType.Flyout and item.actionID then
					local _, _, numSlots = rec("GetFlyoutInfo", item.actionID)
					for i = 1, numSlots or 0 do
						local spellID = rec("GetFlyoutSlotInfo", item.actionID, i)
						if spellID and not seenSpells[spellID] then
							seenSpells[spellID] = true
							rec("C_TooltipInfo.GetSpellByID", spellID)
							rec("C_Spell.GetSpellInfo", spellID)
						end
					end
				end
			end
		end
	end
	for slot = 1, 180 do
		rec("GetActionInfo", slot)
		if C_ActionBar.HasAction(slot) then
			rec("C_TooltipInfo.GetAction", slot)
		end
		for _, name in ipairs({ "HasAction", "GetActionTexture", "GetActionText", "GetActionDisplayCount",
				"GetActionUseCount", "IsUsableAction", "IsConsumableAction", "IsStackableAction",
				"IsAttackAction", "IsAutoRepeatAction", "IsCurrentAction", "IsItemAction",
				"IsEquippedAction", "GetActionCooldown", "GetActionCharges", "GetSpell",
				"HasRangeRequirements", "IsAssistedCombatAction", "IsHarmfulAction", "IsHelpfulAction" }) do
			rec("C_ActionBar." .. name, slot)
		end
	end
	for _, unit in ipairs(UNITS) do
		if UnitExists(unit) then
			rec("UnitExists", unit)
			rec("UnitName", unit)
			rec("UnitGUID", unit)
			rec("UnitLevel", unit)
			rec("UnitEffectiveLevel", unit)
			rec("UnitClass", unit)
			rec("UnitRace", unit)
			rec("UnitSex", unit)
			rec("UnitHealth", unit)
			rec("UnitHealthMax", unit)
			rec("UnitPower", unit)
			rec("UnitPowerMax", unit)
			rec("UnitPowerType", unit)
			rec("UnitReaction", unit, "player")
			rec("UnitIsPlayer", unit)
			rec("UnitIsFriend", "player", unit)
			rec("UnitIsEnemy", "player", unit)
			rec("UnitCanAttack", "player", unit)
			rec("UnitClassification", unit)
			rec("UnitCreatureType", unit)
			rec("UnitFactionGroup", unit)
			rec("UnitIsDead", unit)
			rec("UnitIsGhost", unit)
			rec("UnitIsConnected", unit)
			rec("UnitGroupRolesAssigned", unit)
			rec("UnitSelectionColor", unit)
			rec("UnitInParty", unit)
			rec("UnitInRaid", unit)
			rec("GetRaidTargetIndex", unit)
			for i = 1, 40 do
				if not rec("C_UnitAuras.GetAuraDataByIndex", unit, i, "HELPFUL") then break end
			end
			for i = 1, 40 do
				if not rec("C_UnitAuras.GetAuraDataByIndex", unit, i, "HARMFUL") then break end
			end
		end
	end
	rec("GetNumGroupMembers")
	rec("GetNumSubgroupMembers")
	rec("IsInGroup")
	rec("IsInRaid")
	rec("C_ActionBar.GetActionBarPage")
	rec("C_ActionBar.GetBonusBarOffset")
	rec("C_ActionBar.HasBonusActionBar")
	rec("C_ActionBar.HasOverrideActionBar")
	rec("C_ActionBar.HasVehicleActionBar")
	rec("C_ActionBar.HasTempShapeshiftActionBar")
	rec("C_ActionBar.HasExtraActionBar")
	rec("GetNumShapeshiftForms")
	rec("C_EditMode.GetLayouts")
	rec("C_EditMode.GetAccountSettings")
end

-- Output ---------------------------------------------------------------------

local function characterKey()
	local name, realm = UnitFullName("player")
	return (name or "?") .. "-" .. (realm or GetRealmName() or "?")
end

-- Runs one step of finish; a failing step is logged and the rest go on.
local function step(name, fn, ...)
	local r = pack(pcall(fn, ...))
	if not r[1] then
		table.insert(errors, name .. ": " .. tostring(r[2]))
		return nil
	end
	return unpack(r, 2, r.n)
end

local function captureTrees()
	local trees, atlases = {}, {}
	for _, name in ipairs(FRAME_TREES) do
		local f = _G[name]
		if type(f) == "table" and f.GetObjectType then
			local list = {}
			local ok, err = pcall(recordObject, list, atlases, f, 0)
			if not ok then table.insert(errors, "tree " .. name .. ": " .. tostring(err)) end
			if #list > 0 then trees[name] = list end
		end
	end
	return trees, atlases
end

local function captureMeta(started)
	local version, build, _, tocVersion = GetBuildInfo()
	local physicalWidth, physicalHeight = GetPhysicalScreenSize()
	local classColors = {}
	for class, c in pairs(RAID_CLASS_COLORS or {}) do
		classColors[class] = { r = c.r, g = c.g, b = c.b, a = c.a or 1 }
	end
	return {
		captureVersion = VERSION,
		version = version, build = build, tocVersion = tocVersion,
		locale = GetLocale(),
		character = characterKey(),
		screenWidth = GetScreenWidth(), screenHeight = GetScreenHeight(),
		physicalWidth = physicalWidth, physicalHeight = physicalHeight,
		uiParentScale = UIParent:GetScale(),
		uiParentEffectiveScale = UIParent:GetEffectiveScale(),
		uiScale = GetCVar("uiScale"), useUiScale = GetCVar("useUiScale"),
		spellBookMinimized = PlayerSpellsFrame and PlayerSpellsFrame:IsMinimized() or nil,
		classColors = classColors,
		localizedClasses = LocalizedClassList and LocalizedClassList() or nil,
		localizedClassesFemale = LocalizedClassList and LocalizedClassList(true) or nil,
		hasTarget = UnitExists("target"),
		groupSize = GetNumGroupMembers(),
		seconds = debugprofilestop() - started,
	}
end

local function finish(started)
	stopRecording()

	local trees, atlases = step("trees", captureTrees)
	trees, atlases = trees or {}, atlases or {}
	step("explicit calls", explicitCalls)
	local constants, enums = step("constants", captureConstants)
	data.meta = step("meta", captureMeta, started)
	data.trees = trees
	-- The spellbook tree under its old name, for tools/compare.
	data.frames = trees.PlayerSpellsFrame
	data.atlases = atlases
	data.constants = constants
	data.enums = enums
	data.cvars = step("cvars", captureCVars)
	data.fonts = step("fonts", measureFonts)

	if PlayerSpellsFrame and PlayerSpellsFrame:IsShown() then
		HideUIPanel(PlayerSpellsFrame)
	end

	local okPack, packed = pcall(function()
		local json = C_EncodingUtil.SerializeJSON(plain(data, {}))
		return C_EncodingUtil.EncodeBase64(C_EncodingUtil.CompressString(json))
	end)
	if not okPack then
		print("|cff33ff99WowProgramming|r capture failed to encode the data: " .. tostring(packed))
		return
	end
	WowProgrammingCaptureDB[characterKey()] = {
		version = VERSION,
		time = time(),
		data = packed,
	}
	local n = 0
	for _ in pairs(calls) do n = n + 1 end
	print(string.format(
		"|cff33ff99WowProgramming|r capture done: %d functions, %d frame trees, %d errors, %d KB. Type /reload to save it.",
		n, (function() local c = 0 for _ in pairs(trees) do c = c + 1 end return c end)(),
		#errors, math.floor(#packed / 1024)))
end

local function capture()
	if InCombatLockdown() then
		print("|cff33ff99WowProgramming|r capture: leave combat first.")
		return
	end
	local started = debugprofilestop()
	calls, errors = {}, {}
	data = { calls = calls, errors = errors }
	startRecording()
	print(string.format("|cff33ff99WowProgramming|r capture: recording %d functions...", #originals))
	local handled = refreshFrames()
	table.insert(errors, 1, string.format("info: %d event handler calls", handled))
	-- Open the spellbook on the class tab so its calls and layout are recorded.
	if PlayerSpellsUtil and PlayerSpellsUtil.OpenToSpellBookTab then
		local ok, err = pcall(PlayerSpellsUtil.OpenToSpellBookTab)
		if not ok then table.insert(errors, "spellbook: " .. tostring(err)) end
	end
	C_Timer.After(1.5, function()
		finish(started)
	end)
end

SLASH_WOWPROGRAMMINGCAPTURE1 = "/wpcapture"
SlashCmdList.WOWPROGRAMMINGCAPTURE = capture
