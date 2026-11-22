-- Helpers for the control bridge. Each returns a short text answer.

local function isWidget(v)
    return type(v) == "table" and type(rawget(v, 0)) == "userdata" and v.GetObjectType ~= nil
end

local function short(v, depth)
    depth = depth or 0
    local t = type(v)
    if t == "string" then
        return string.format("%q", v)
    elseif isWidget(v) then
        return "<" .. v:GetObjectType() .. " " .. v:GetDebugName() .. ">"
    elseif t == "table" then
        if depth >= 2 then return "{...}" end
        local parts, n = {}, 0
        for k, val in pairs(v) do
            n = n + 1
            if n > 25 then parts[#parts + 1] = "..."; break end
            parts[#parts + 1] = tostring(k) .. "=" .. short(val, depth + 1)
        end
        return "{" .. table.concat(parts, ", ") .. "}"
    end
    return tostring(v)
end

local function resolve(expr)
    local f, err = loadstring("return " .. expr, "=ctl")
    if not f then return nil, err end
    local ok, v = pcall(f)
    if not ok then return nil, tostring(v) end
    return v
end

function Inspect_Eval(expr)
    local f, err = loadstring("return " .. expr, "=ctl")
    if not f then return "error: " .. err end
    local r = { pcall(f) }
    if not r[1] then return "error: " .. tostring(r[2]) end
    local out = {}
    for i = 2, table.maxn(r) do out[#out + 1] = short(r[i]) end
    if #out == 0 then return "(no values)" end
    return table.concat(out, "\n")
end

local function screenRect(f)
    local l, b, w, h = f:GetRect()
    if not l then return nil end
    local s = f:GetEffectiveScale()
    return l * s, b * s, w * s, h * s
end

function Inspect_Center(expr)
    local f, err = resolve(expr)
    if not isWidget(f) then return "error: not a widget: " .. tostring(err or f) end
    if not f:IsVisible() then return "error: " .. f:GetDebugName() .. " is not visible" end
    local l, b, w, h = screenRect(f)
    if not l then return "error: no rectangle" end
    return string.format("%f %f", l + w / 2, b + h / 2)
end

local function describe(f)
    local l, b, w, h = screenRect(f)
    local rect = l and string.format("[%.0f,%.0f %.0fx%.0f]", l, b, w, h) or "[no rect]"
    local extra = ""
    local t = f:GetObjectType()
    if t == "FontString" or (f.GetText and t ~= "Texture") then
        local ok, text = pcall(f.GetText, f)
        if ok and type(text) == "string" and text ~= "" then extra = extra .. " text=" .. string.format("%q", text:sub(1, 60)) end
    end
    if t == "Texture" then
        local atlas = f:GetAtlas()
        if atlas then extra = extra .. " atlas=" .. atlas else extra = extra .. " tex=" .. tostring(f:GetTexture()) end
    end
    if f.GetFrameLevel then extra = extra .. " lvl=" .. f:GetFrameLevel() end
    if f.IsMouseEnabled and f:IsMouseEnabled() then extra = extra .. " mouse" end
    return t .. " " .. f:GetDebugName() .. " " .. rect .. extra
end

function Inspect_Under(x, y)
    if not x then x, y = GetCursorPosition() end
    local out = {}
    local f = EnumerateFrames()
    while f do
        if f:IsVisible() then
            local l, b, w, h = screenRect(f)
            if l and x >= l and x <= l + w and y >= b and y <= b + h then
                out[#out + 1] = describe(f)
            end
        end
        f = EnumerateFrames(f)
    end
    if #out == 0 then return "nothing at " .. x .. " " .. y end
    return table.concat(out, "\n")
end

function Inspect_Tree(expr, maxDepth)
    local root, err = resolve(expr)
    if not isWidget(root) then return "error: not a widget: " .. tostring(err or root) end
    local out, count = {}, 0
    local function walk(f, depth)
        count = count + 1
        if count > 400 then return end
        out[#out + 1] = string.rep("  ", depth) .. describe(f)
        if depth >= maxDepth then return end
        if f.GetRegions then
            for _, r in ipairs({ f:GetRegions() }) do
                if r:IsShown() then
                    count = count + 1
                    out[#out + 1] = string.rep("  ", depth + 1) .. describe(r)
                end
            end
        end
        if f.GetChildren then
            for _, c in ipairs({ f:GetChildren() }) do
                if c:IsShown() then walk(c, depth + 1) end
            end
        end
    end
    walk(root, 0)
    if count > 400 then out[#out + 1] = "... (truncated)" end
    return table.concat(out, "\n")
end
