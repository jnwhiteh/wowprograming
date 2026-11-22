-- Helpers for scenario files. Called with the scenario folder reader and
-- the decoders; returns the table that scenario.lua receives as its second
-- argument.
local readFile, decodeBase64, decompress, decodeJSON = ...
local type, pairs, ipairs, select, tostring, tonumber, unpack, rawget, _G =
      type, pairs, ipairs, select, tostring, tonumber, unpack, rawget, _G

local S = {}

-- Read a capture file: JSON, or the compressed Base64 text that the
-- in-game capture scripts produce.
function S.LoadCapture(name)
    local text = readFile(name)
    if not text then return nil end
    text = text:gsub("^%s+", ""):gsub("%s+$", "")
    if text:sub(1, 1) ~= "{" then
        local raw = decodeBase64(text)
        text = raw and decompress(raw)
        if not text then error("cannot decode capture " .. name) end
    end
    return decodeJSON(text)
end

local function argKey(...)
    local parts = {}
    for i = 1, select("#", ...) do parts[i] = tostring((select(i, ...))) end
    return table.concat(parts, ",")
end

local function unpackRecord(rec)
    local n = tonumber(rec.n) or 0
    local out = {}
    for i = 1, n do
        local v = rec[tostring(i)]
        if v == nil then v = rec[i] end
        out[i] = v
    end
    return unpack(out, 1, n)
end

-- Path "C_Spell.GetSpellInfo" -> container table and key.
local function locate(path)
    local container, key = _G, path
    for part, rest in path:gmatch("([^.]+)%.(.+)") do
        local t = rawget(container, part)
        if type(t) ~= "table" then
            t = {}
            container[part] = t
        end
        container, key = t, rest
    end
    return container, key
end

S.replayed = {}

-- Replace functions with recorded answers. Calls with arguments that were
-- not recorded fall back to the previous function (usually a stub).
function S.Replay(calls)
    local count = 0
    for path, records in pairs(calls) do
        local container, key = locate(path)
        local fallback = container[key]
        container[key] = function(...)
            local rec = records[argKey(...)]
            if rec == nil then
                -- Try without trailing nil arguments.
                local n = select("#", ...)
                local args = { ... }
                while n > 0 and args[n] == nil do n = n - 1 end
                rec = records[argKey(unpack(args, 1, n))]
            end
            if rec ~= nil then return unpackRecord(rec) end
            if fallback then return fallback(...) end
        end
        S.replayed[path] = true
        count = count + 1
    end
    return count
end

-- Define a function or value at a path such as "C_Spell.IsSpellDataCached".
function S.Set(path, value)
    local container, key = locate(path)
    container[key] = value
end

-- Merge client constants: colors, Enum, Constants and number globals.
function S.ApplyClientConstants(data)
    if data.colors then
        local colors = {}
        for i, c in ipairs(data.colors) do
            colors[i] = { baseTag = c.baseTag, color = { r = c.r, g = c.g, b = c.b, a = c.a } }
        end
        C_UIColor.GetColors = function() return colors end
    end
    local function merge(target, source)
        for k, v in pairs(source) do
            local key = tonumber(k) or k
            if type(v) == "table" then
                if type(target[key]) ~= "table" then target[key] = {} end
                merge(target[key], v)
            elseif target[key] == nil then
                target[key] = v
            end
        end
    end
    if data.enum then merge(Enum, data.enum) end
    if data.constants then merge(Constants, data.constants) end
    if data.numbers then
        for k, v in pairs(data.numbers) do
            if rawget(_G, k) == nil then _G[k] = v end
        end
    end
end

return S
