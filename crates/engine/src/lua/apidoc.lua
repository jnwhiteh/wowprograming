-- Builds globals from the API documentation files:
--   Enum and Constants, and stubs for documented functions and widget
--   methods that return typed default values.
-- Arguments: a list of { name, code } pairs for the documentation files,
-- the function that compiles a chunk (loadstring), and a table that records
-- the names of stubs when they are called.
-- Returns Enum, Constants, a list of error messages, the function stubs
-- (global name -> function, or namespace name -> table of functions) and
-- the widget method stubs (documentation table name -> table of functions).

local files, loadstring, calledStubs = ...
local setmetatable, setfenv, pcall, ipairs, pairs, table, tostring, type, select, unpack =
      setmetatable, setfenv, pcall, ipairs, pairs, table, tostring, type, select, unpack

-- Run every file with APIDocumentation:AddDocumentationTable collecting the
-- tables, and the given values as Enum and Constants.
local function collect(enum, constants, errors)
    local docs = {}
    local env = {
        Enum = enum,
        Constants = constants,
        APIDocumentation = {
            AddDocumentationTable = function(_, t) table.insert(docs, t) end,
        },
    }
    for _, file in ipairs(files) do
        local chunk, err = loadstring(file[2], "@" .. file[1])
        if chunk then
            setfenv(chunk, env)
            local ok, runErr = pcall(chunk)
            if not ok and errors then table.insert(errors, tostring(runErr)) end
        elseif errors then
            table.insert(errors, err)
        end
    end
    return docs
end

-- Files refer to Enum and Constants entries defined in other files. The
-- first passes use a placeholder that accepts any field.
local placeholderEntry = setmetatable({}, { __index = function() return 0 end })
local placeholder = setmetatable({}, { __index = function() return placeholderEntry end })

-- Pass 1: enumerations.
local Enum = {}
for _, doc in ipairs(collect(placeholder, placeholder, nil)) do
    for _, t in ipairs(doc.Tables or {}) do
        if t.Type == "Enumeration" then
            local values = {}
            for _, field in ipairs(t.Fields or {}) do
                values[field.Name] = field.EnumValue
            end
            Enum[t.Name] = values
            Enum[t.Name .. "Meta"] = {
                NumValues = t.NumValues,
                MinValue = t.MinValue,
                MaxValue = t.MaxValue,
            }
        end
    end
end

-- A constant whose type is an enumeration may name a member as a string.
local function resolve(v)
    local enum = Enum[v.Type]
    if type(v.Value) == "string" and enum and enum[v.Value] ~= nil then
        return enum[v.Value]
    end
    return v.Value
end

local finalDocs
local function collectConstants(constantsEnv, errors)
    local result = {}
    local docs = collect(Enum, constantsEnv, errors)
    for _, doc in ipairs(docs) do
        for _, t in ipairs(doc.Tables or {}) do
            if t.Type == "Constants" then
                local values = result[t.Name] or {}
                for _, v in ipairs(t.Values or {}) do
                    values[v.Name] = resolve(v)
                end
                result[t.Name] = values
            end
        end
    end
    finalDocs = docs
    return result
end

-- Pass 2: constants, with placeholders for references to other constants.
-- Pass 3: constants again, with the results of pass 2.
local errors = {}
local Constants = collectConstants(collectConstants(placeholder, nil), errors)

-- Stubs ----------------------------------------------------------------------

local structures = {}
for _, doc in ipairs(finalDocs) do
    for _, t in ipairs(doc.Tables or {}) do
        if t.Type == "Structure" then structures[t.Name] = t end
    end
end

local numberTypes = {
    number = true, luaIndex = true, size = true, uiUnit = true, fileID = true,
    time_t = true, BigUInteger = true, BigInteger = true, Seconds = true,
    normalizedValue = true, SingleColorValue = true, FragmentID = true,
    BagIndex = true, uiMapID = true, Milliseconds = true,
    float = true, double = true, int = true,
}
local stringTypes = {
    string = true, cstring = true, WOWGUID = true, textureAtlas = true,
    textureKit = true, mouseButton = true, stringView = true, ClubId = true,
    ClubStreamId = true, UnitToken = true, UnitTokenVariant = true,
    FramePoint = true, FrameStrata = true, DrawLayer = true, kstringClubId = true,
}

local default
local widgetStubs
local objectMetatables = {}
local function structDefault(def, depth)
    local t = {}
    for _, field in ipairs(def.Fields or {}) do
        t[field.Name] = default(field, depth + 1)
    end
    return t
end

-- A value with the documented mixin (ColorMixin, Vector2DMixin, ...), when
-- Blizzard's Lua has defined it.
local function withMixin(value, mixinName)
    local mixin = mixinName and rawget(_G, mixinName)
    if type(value) ~= "table" or type(mixin) ~= "table" then return value end
    for k, v in pairs(mixin) do
        if value[k] == nil then value[k] = v end
    end
    return value
end

function default(arg, depth)
    if arg.Nilable then return nil end
    if arg.Default ~= nil and type(arg.Default) ~= "table" then return arg.Default end
    local ty = arg.Type
    if ty == "colorRGBA" or ty == "colorRGB" then
        -- White, so code that reads the colour keeps working.
        return withMixin({ r = 1, g = 1, b = 1, a = 1 }, arg.Mixin or "ColorMixin")
    end
    if arg.Mixin then
        local t = ty ~= arg.Mixin and structures[ty] and depth < 4 and structDefault(structures[ty], depth) or {}
        return withMixin(t, arg.Mixin)
    end
    if ty == "table" then return {} end
    if ty == "bool" then return false end
    if numberTypes[ty] then return 0 end
    if stringTypes[ty] then return "" end
    local meta = Enum[ty .. "Meta"]
    if Enum[ty] then return meta and meta.MinValue or 0 end
    local s = structures[ty]
    if s and depth < 4 then return structDefault(s, depth) end
    -- A documented object type (SecondsFormatter, for example): an object
    -- whose methods are stubs too.
    local methods = widgetStubs and widgetStubs[ty .. "API"]
    if methods then
        local mt = objectMetatables[ty]
        if not mt then
            mt = { __index = methods }
            objectMetatables[ty] = mt
        end
        return setmetatable({}, mt)
    end
    return nil
end

local function makeStub(fullName, fn)
    local returns = {}
    for i, r in ipairs(fn.Returns or {}) do returns[i] = r end
    local n = #returns
    return function()
        calledStubs[fullName] = (calledStubs[fullName] or 0) + 1
        if n == 0 then return end
        local out = {}
        for i = 1, n do out[i] = default(returns[i], 0) end
        return unpack(out, 1, n)
    end
end

local functionStubs = {}
widgetStubs = {}
for _, doc in ipairs(finalDocs) do
    if doc.Type == "ScriptObject" then
        local methods = widgetStubs[doc.Name] or {}
        for _, fn in ipairs(doc.Functions or {}) do
            methods[fn.Name] = makeStub(doc.Name .. ":" .. fn.Name, fn)
        end
        widgetStubs[doc.Name] = methods
    elseif doc.Namespace then
        local ns = functionStubs[doc.Namespace] or {}
        for _, fn in ipairs(doc.Functions or {}) do
            ns[fn.Name] = makeStub(doc.Namespace .. "." .. fn.Name, fn)
        end
        functionStubs[doc.Namespace] = ns
    else
        for _, fn in ipairs(doc.Functions or {}) do
            functionStubs[fn.Name] = makeStub(fn.Name, fn)
        end
    end
end

-- Mixins of documented return values, for restoring methods on recorded
-- results: full name -> { [index] = shape }. A shape is
-- { mixin = name, fields = { [field] = shape }, each = shape }: the
-- value's own mixin, the shapes of its structure fields, and the shape of
-- every element of a list. Only parts with a mixin somewhere are kept.
local function shapeOf(arg, depth)
    if depth > 5 then return nil end
    if arg.Type == "table" and arg.InnerType then
        local inner = shapeOf({ Type = arg.InnerType }, depth + 1)
        return inner and { each = inner } or nil
    end
    local fields
    local s = structures[arg.Type]
    if s then
        for _, field in ipairs(s.Fields or {}) do
            local sh = shapeOf(field, depth + 1)
            if sh then
                fields = fields or {}
                fields[field.Name] = sh
            end
        end
    end
    if not arg.Mixin and not fields then return nil end
    return { mixin = arg.Mixin, fields = fields }
end

local returnMixins = {}
for _, doc in ipairs(finalDocs) do
    if doc.Type ~= "ScriptObject" then
        for _, fn in ipairs(doc.Functions or {}) do
            local full = doc.Namespace and (doc.Namespace .. "." .. fn.Name) or fn.Name
            for i, r in ipairs(fn.Returns or {}) do
                local sh = shapeOf(r, 0)
                if sh then
                    returnMixins[full] = returnMixins[full] or {}
                    returnMixins[full][i] = sh
                end
            end
        end
    end
end

-- Events: every documented event is valid.
local events = {}
for _, doc in ipairs(finalDocs) do
    for _, e in ipairs(doc.Events or {}) do
        if e.LiteralName then events[e.LiteralName] = true end
    end
end
functionStubs.C_EventUtils = functionStubs.C_EventUtils or {}
functionStubs.C_EventUtils.IsEventValid = function(name)
    return events[name] == true
end

return Enum, Constants, errors, functionStubs, widgetStubs, returnMixins
