-- Builds the Enum and Constants globals from the API documentation files.
-- Arguments: a list of { name, code } pairs for the documentation files,
-- and the function that compiles a chunk (loadstring).
-- Returns Enum, Constants and a list of error messages.

local files, loadstring = ...
local setmetatable, setfenv, pcall, ipairs, table, tostring, type =
      setmetatable, setfenv, pcall, ipairs, table, tostring, type

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

local function collectConstants(constantsEnv, errors)
    local result = {}
    for _, doc in ipairs(collect(Enum, constantsEnv, errors)) do
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
    return result
end

-- Pass 2: constants, with placeholders for references to other constants.
-- Pass 3: constants again, with the results of pass 2.
local errors = {}
local Constants = collectConstants(collectConstants(placeholder, nil), errors)

return Enum, Constants, errors
