-- Functions the client adds natively to the Lua libraries and a few small
-- utility namespaces (see LuaTableExtensions, LuaMathExtensions,
-- LuaStringExtensions, ColorUtil and Localization in the API docs).
local pairs, next, type, select, tostring, rawset, setmetatable, getmetatable =
      pairs, next, type, select, tostring, rawset, setmetatable, getmetatable
local floor, huge, format = math.floor, math.huge, string.format

-- table
function table.contains(t, value)
    for _, v in pairs(t) do if v == value then return true end end
    return false
end
function table.count(t)
    local n = 0
    for _ in pairs(t) do n = n + 1 end
    return n
end
function table.create() return {} end
function table.indexof(t, value)
    for i = 1, #t do if t[i] == value then return i end end
    return nil
end
local frozen = setmetatable({}, { __mode = "k" })
function table.freeze(t)
    frozen[t] = true
    return t
end
function table.isfrozen(t) return frozen[t] == true end
function table.keys(t)
    local out = {}
    for k in pairs(t) do out[#out + 1] = k end
    return out
end
function table.values(t)
    local out = {}
    for _, v in pairs(t) do out[#out + 1] = v end
    return out
end
function table.getcountinfo(t)
    local n, ints, maxInt = 0, 0, 0
    for k in pairs(t) do
        n = n + 1
        if type(k) == "number" and k > 0 and floor(k) == k then
            ints = ints + 1
            if k > maxInt then maxInt = k end
        end
    end
    return n, ints, maxInt
end
function table.isempty(t) return next(t) == nil end
function table.removeunordered(t, index)
    local n = #t
    index = index or n
    if index < 1 or index > n then return nil end
    local v = t[index]
    t[index] = t[n]
    t[n] = nil
    return v
end
function table.removevalue(t, value)
    local removed, i = 0, 1
    while i <= #t do
        if t[i] == value then
            table.remove(t, i)
            removed = removed + 1
        else
            i = i + 1
        end
    end
    return removed
end

-- math
function math.clamp(v, lo, hi) if v < lo then return lo elseif v > hi then return hi end return v end
function math.isnan(v) return v ~= v end
function math.isinf(v) return v == huge or v == -huge end
function math.isfinite(v) return v == v and v ~= huge and v ~= -huge end
function math.lerp(a, b, t) return a + (b - a) * t end
function math.normalize(v, lo, hi) if hi == lo then return 0 end return (v - lo) / (hi - lo) end
function math.remap(v, a, b, c, d) if b == a then return c end return c + (v - a) * (d - c) / (b - a) end
function math.round(v, places)
    local m = 10 ^ (places or 0)
    return floor(v * m + 0.5) / m
end
function math.saturate(v) return math.clamp(v, 0, 1) end
function math.sign(v) if v > 0 then return 1 elseif v < 0 then return -1 end return 0 end
function math.wrap(v, lo, hi)
    if hi == lo then return lo end
    return lo + (v - lo) % (hi - lo)
end

-- string
local function charClass(chars)
    chars = chars or " \r\n\t"
    return "[" .. chars:gsub("[%]%%%^%-]", "%%%0") .. "]"
end
function string.ltrim(s, chars) return (s:gsub("^" .. charClass(chars) .. "+", "")) end
function string.rtrim(s, chars) return (s:gsub(charClass(chars) .. "+$", "")) end
if not string.trim then
    function string.trim(s, chars) return string.rtrim(string.ltrim(s, chars), chars) end
end
function string.contains(s, sub) return s:find(sub, 1, true) ~= nil end
function string.startswith(s, prefix) return s:sub(1, #prefix) == prefix end
function string.endswith(s, suffix) return suffix == "" or s:sub(-#suffix) == suffix end

-- C_ColorUtil
C_ColorUtil = C_ColorUtil or {}
local function byte(v) v = floor((v or 0) * 255 + 0.5); if v < 0 then v = 0 elseif v > 255 then v = 255 end return v end
function C_ColorUtil.GenerateTextColorCode(color)
    return format("ff%02x%02x%02x", byte(color.r), byte(color.g), byte(color.b))
end
function C_ColorUtil.WrapTextInColorCode(text, hex)
    return "|c" .. hex .. tostring(text) .. "|r"
end
function C_ColorUtil.WrapTextInColor(text, color)
    return C_ColorUtil.WrapTextInColorCode(text, C_ColorUtil.GenerateTextColorCode(color))
end
function C_ColorUtil.ConvertRGBToHSV(r, g, b)
    local max, min = math.max(r, g, b), math.min(r, g, b)
    local d, h = max - min, 0
    if d > 0 then
        if max == r then h = ((g - b) / d) % 6
        elseif max == g then h = (b - r) / d + 2
        else h = (r - g) / d + 4 end
        h = h * 60
    end
    return h, max == 0 and 0 or d / max, max
end
function C_ColorUtil.ConvertHSVToRGB(h, s, v)
    local c = v * s
    local x = c * (1 - math.abs((h / 60) % 2 - 1))
    local m = v - c
    local r, g, b
    if h < 60 then r, g, b = c, x, 0 elseif h < 120 then r, g, b = x, c, 0
    elseif h < 180 then r, g, b = 0, c, x elseif h < 240 then r, g, b = 0, x, c
    elseif h < 300 then r, g, b = x, 0, c else r, g, b = c, 0, x end
    return r + m, g + m, b + m
end
function C_ColorUtil.ConvertHSVToHSL(h, s, v)
    local l = v * (1 - s / 2)
    local sl = (l == 0 or l == 1) and 0 or (v - l) / math.min(l, 1 - l)
    return h, sl, l
end
function C_ColorUtil.ConvertHSLToHSV(h, s, l)
    local v = l + s * math.min(l, 1 - l)
    return h, v == 0 and 0 or 2 * (1 - l / v), v
end

-- Class colors and names need game data that is not captured yet.
C_ClassColor = C_ClassColor or {}
function C_ClassColor.GetClassColor()
    if CreateColor then return CreateColor(1, 1, 1, 1) end
    return { r = 1, g = 1, b = 1, a = 1 }
end
function LocalizedClassList()
    local list = {}
    if UnitClass then
        local name, file = UnitClass("player")
        if file then list[file] = name end
    end
    return list
end

-- Number formatting from Localization.
function BreakUpLargeNumbers(n)
    local s = tostring(floor(n or 0))
    local out = s:reverse():gsub("(%d%d%d)", "%1,"):reverse()
    return (out:gsub("^,", ""):gsub("^%-,", "-"))
end
function AbbreviateLargeNumbers(n)
    n = n or 0
    if n >= 1e6 then return format("%.1fM", n / 1e6) end
    if n >= 1e4 then return format("%dK", floor(n / 1e3)) end
    return BreakUpLargeNumbers(n)
end
AbbreviateNumbers = AbbreviateLargeNumbers
