-- Client constants capture for WowProgramming.
--
-- Paste into WowLua (or any in-game Lua runner) and run it. It records
-- values that the game client defines natively and that are not in the UI
-- source or the API documentation:
--
--   colors    = C_UIColor.GetColors()            (named colors such as NORMAL_FONT_COLOR)
--   enum      = the Enum table                   (some enums, such as Enum.GameMode, are not documented)
--   constants = the Constants table
--   numbers   = every global number or boolean   (LE_* and NUM_* constants and similar)
--
-- It shows the result in a window as one compressed text line. Press Ctrl+C
-- to copy it and save it to a file.

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

local data = {
	colors = colors,
	enum = copyTable(Enum, 1),
	constants = copyTable(Constants, 1),
	numbers = numbers,
}
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
	"Captured %d colors and client constants.\n%d characters. Press Ctrl+C to copy, then Escape to close.",
	#colors, #packed))
f.EditBox:SetText(packed)
f.EditBox:SetFocus()
f.EditBox:HighlightText()
