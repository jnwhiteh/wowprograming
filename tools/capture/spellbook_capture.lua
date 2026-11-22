-- Spellbook scenario capture for WowProgramming.
--
-- Paste into WowLua (or any in-game Lua runner) and run it out of combat.
-- It records API calls and their return values, opens the spellbook, and
-- records the position of every shown part of the spellbook frame. Then it
-- shows the result in a window as one compressed text line. Press Ctrl+C to
-- copy it and save it to a file.
--
-- Output shape (before compression), all keys are strings so that nil
-- return values leave no holes:
--   meta  = { build, locale, screen size, UI scale, ... }
--   calls = { ["C_SpellBook.GetSpellBookItemInfo"] = { ["12,0"] = { n = 1, ["1"] = {...} } } }
--   atlases = { [atlasName] = C_Texture.GetAtlasInfo(atlasName) }
--   frames = { { path, type, rect, ... }, ... }   -- the opened spellbook
--   errors = { "..." }

local calls, errors, atlases, frames = {}, {}, {}, {}

local function argKey(...)
	local parts = {}
	for i = 1, select("#", ...) do
		parts[i] = tostring((select(i, ...)))
	end
	return table.concat(parts, ",")
end

local function resolve(name)
	local value = _G
	for part in name:gmatch("[^.]+") do
		if type(value) ~= "table" then
			return nil
		end
		value = value[part]
	end
	return value
end

local function isSecret(v)
	return issecretvalue and issecretvalue(v)
end

-- Calls the named function, records its return values, and returns them.
local function rec(name, ...)
	local fn = resolve(name)
	if type(fn) ~= "function" then
		table.insert(errors, "missing function " .. name)
		return
	end
	local r = { n = 0 }
	local function pack(ok, ...)
		r.ok, r.n = ok, select("#", ...)
		for i = 1, r.n do
			r[i] = (select(i, ...))
		end
	end
	pack(pcall(fn, ...))
	if not r.ok then
		table.insert(errors, name .. "(" .. argKey(...) .. "): " .. tostring(r[1]))
		return
	end
	local out = { n = r.n }
	for i = 1, r.n do
		if isSecret(r[i]) then
			out[tostring(i)] = "<secret>"
		else
			out[tostring(i)] = r[i]
		end
	end
	calls[name] = calls[name] or {}
	calls[name][argKey(...)] = out
	return unpack(r, 1, r.n)
end

-- Player and specialization ---------------------------------------------

rec("UnitClass", "player")
rec("UnitRace", "player")
rec("UnitSex", "player")
rec("UnitLevel", "player")
rec("UnitFactionGroup", "player")
rec("GetLocale")
rec("GetBindingKey", "TOGGLESPELLBOOK")
local _, _, classID = UnitClass("player")
rec("C_CreatureInfo.GetClassInfo", classID)
rec("GetClassInfo", classID)
rec("GetSpecialization")
rec("GetSpecialization", false, false)
rec("C_SpecializationInfo.GetSpecialization")
rec("C_SpecializationInfo.IsInitialized")
rec("C_SpecializationInfo.CanPlayerUseTalentSpecUI")
local numSpecs = rec("GetNumSpecializations") or 0
rec("GetNumSpecializations", false, false)
rec("GetNumSpecializationsForClassID", classID)
for i = 1, numSpecs do
	local specID = rec("GetSpecializationInfo", i)
	rec("C_SpecializationInfo.GetSpecializationInfo", i)
	rec("GetSpecializationInfoForClassID", classID, i)
	if specID then
		rec("GetSpecializationInfoByID", specID)
		rec("C_SpecializationInfo.GetClassIDFromSpecID", specID)
	end
end

-- Spellbook ----------------------------------------------------------------

local seenSpells = {}
local function recordSpell(spellID)
	if not spellID or seenSpells[spellID] then
		return
	end
	seenSpells[spellID] = true
	rec("C_Spell.GetSpellInfo", spellID)
	rec("C_Spell.GetSpellName", spellID)
	rec("C_Spell.GetSpellTexture", spellID)
	rec("C_Spell.GetSpellSubtext", spellID)
	rec("C_Spell.GetOverrideSpell", spellID)
	rec("C_Spell.IsSpellPassive", spellID)
	rec("C_Spell.IsAutoAttackSpell", spellID)
	rec("C_Spell.IsRangedAutoAttackSpell", spellID)
	rec("C_LevelLink.IsSpellLocked", spellID)
	rec("C_ActionBar.FindSpellActionButtons", spellID)
	rec("C_ActionBar.IsOnBarOrSpecialBar", spellID)
	rec("C_ClickBindings.CanSpellBeClickBound", spellID)
	rec("C_SpellBook.IsSpellInSpellBook", spellID)
	rec("C_SpellBook.FindSpellBookSlotForSpell", spellID)
end

local function recordSlot(slot, bank)
	local info = rec("C_SpellBook.GetSpellBookItemInfo", slot, bank)
	rec("C_SpellBook.GetSpellBookItemTexture", slot, bank)
	rec("C_SpellBook.GetSpellBookItemLevelLearned", slot, bank)
	rec("C_SpellBook.IsSpellBookItemPassive", slot, bank)
	rec("C_SpellBook.GetSpellBookItemType", slot, bank)
	rec("C_SpellBook.GetSpellBookItemCooldown", slot, bank)
	rec("C_SpellBook.GetSpellBookItemAutoCast", slot, bank)
	if not info then
		return
	end
	recordSpell(info.spellID)
	if info.itemType == Enum.SpellBookItemType.Flyout and info.actionID then
		local _, _, numSlots = rec("GetFlyoutInfo", info.actionID)
		for i = 1, numSlots or 0 do
			local spellID = rec("GetFlyoutSlotInfo", info.actionID, i)
			recordSpell(spellID)
		end
	end
end

local playerBank = Enum.SpellBookSpellBank.Player
local numLines = rec("C_SpellBook.GetNumSpellBookSkillLines") or 0
rec("C_SpellBook.HasPetSpells")
-- The spellbook also asks for skill lines past the last one, so record a few extra.
for i = 1, numLines + 5 do
	local line = rec("C_SpellBook.GetSpellBookSkillLineInfo", i)
	if line and line.itemIndexOffset and line.numSpellBookItems then
		for slot = line.itemIndexOffset + 1, line.itemIndexOffset + line.numSpellBookItems do
			recordSlot(slot, playerBank)
		end
	end
end

-- Meta -----------------------------------------------------------------------

local version, build, date, tocVersion = GetBuildInfo()
local physicalWidth, physicalHeight = GetPhysicalScreenSize()
local meta = {
	version = version,
	build = build,
	tocVersion = tocVersion,
	locale = GetLocale(),
	screenWidth = GetScreenWidth(),
	screenHeight = GetScreenHeight(),
	physicalWidth = physicalWidth,
	physicalHeight = physicalHeight,
	uiParentScale = UIParent:GetScale(),
	uiParentEffectiveScale = UIParent:GetEffectiveScale(),
	uiScale = GetCVar("uiScale"),
	useUiScale = GetCVar("useUiScale"),
}

-- Frame positions of the opened spellbook -------------------------------------

local function round(v)
	return v and math.floor(v * 100 + 0.5) / 100
end

local function recordObject(obj, depth)
	if #frames >= 4000 or not obj:IsShown() then
		return
	end
	local entry = {
		path = obj.GetDebugName and obj:GetDebugName() or "?",
		type = obj:GetObjectType(),
		depth = depth,
		alpha = round(obj:GetAlpha()),
	}
	local left, bottom, width, height = obj:GetRect()
	if left then
		entry.rect = { round(left), round(bottom), round(width), round(height) }
	end
	if obj.GetFrameLevel then
		entry.level = obj:GetFrameLevel()
		entry.strata = obj:GetFrameStrata()
	end
	if obj.GetDrawLayer then
		local layer, sublevel = obj:GetDrawLayer()
		entry.layer, entry.sublevel = layer, sublevel
	end
	if entry.type == "Texture" then
		local atlas = obj:GetAtlas()
		if atlas then
			entry.atlas = atlas
			if not atlases[atlas] then
				atlases[atlas] = C_Texture.GetAtlasInfo(atlas) or false
			end
		end
		entry.texture = obj:GetTexture()
		entry.texCoord = { obj:GetTexCoord() }
		for i, v in ipairs(entry.texCoord) do
			entry.texCoord[i] = round(v)
		end
	elseif entry.type == "FontString" then
		entry.text = obj:GetText()
		entry.font = obj:GetFont()
	end
	table.insert(frames, entry)

	if obj.GetRegions then
		for _, region in ipairs({ obj:GetRegions() }) do
			recordObject(region, depth + 1)
		end
	end
	if obj.GetChildren and depth < 16 then
		for _, child in ipairs({ obj:GetChildren() }) do
			recordObject(child, depth + 1)
		end
	end
end

local function finish()
	if PlayerSpellsFrame and PlayerSpellsFrame:IsShown() then
		meta.spellBookMinimized = PlayerSpellsFrame:IsMinimized()
		recordObject(PlayerSpellsFrame, 0)
	else
		table.insert(errors, "PlayerSpellsFrame did not open")
	end

	local data = { meta = meta, calls = calls, atlases = atlases, frames = frames, errors = errors }
	local json = C_EncodingUtil.SerializeJSON(data)
	local packed = C_EncodingUtil.EncodeBase64(C_EncodingUtil.CompressString(json))

	local f = WowProgrammingCaptureFrame or CreateFrame("Frame", "WowProgrammingCaptureFrame", UIParent, "BasicFrameTemplateWithInset")
	f:SetSize(640, 200)
	f:SetPoint("CENTER")
	f:SetFrameStrata("DIALOG")
	f:Show()
	if not f.EditBox then
		f.Info = f:CreateFontString(nil, "OVERLAY", "GameFontHighlight")
		f.Info:SetPoint("TOPLEFT", 16, -32)
		f.Info:SetPoint("TOPRIGHT", -16, -32)
		f.Info:SetJustifyH("LEFT")
		f.EditBox = CreateFrame("EditBox", nil, f, "InputBoxTemplate")
		f.EditBox:SetPoint("BOTTOMLEFT", 20, 20)
		f.EditBox:SetPoint("BOTTOMRIGHT", -16, 20)
		f.EditBox:SetHeight(24)
		f.EditBox:SetAutoFocus(false)
		f.EditBox:SetMaxLetters(0)
		f.EditBox:SetScript("OnEscapePressed", function() f:Hide() end)
	end
	f.Info:SetText(string.format(
		"Captured %d spellbook frame parts, %d atlases, %d errors.\n%d characters. Press Ctrl+C to copy, then Escape to close.",
		#frames, (function() local n = 0 for _ in pairs(atlases) do n = n + 1 end return n end)(), #errors, #packed))
	f.EditBox:SetText(packed)
	f.EditBox:SetFocus()
	f.EditBox:HighlightText()
end

-- Open the spellbook on the class tab, page 1, then wait so it can lay out.
if InCombatLockdown() then
	print("WowProgramming capture: leave combat first.")
else
	PlayerSpellsUtil.OpenToSpellBookTab()
	C_Timer.After(1, finish)
end
