-- Runs TomTom's own in-game checks (the TomTomTest addon) without a
-- person: each step's function in order, then the test's checks. Steps
-- that ask the tester to do something by hand are not done, so some
-- checks fail for that reason; the list shows what the sandbox cannot do
-- yet. Tests with a reload step are skipped.
-- wowprogramming test --scenario scenarios/dummy --addon TomTom --addon TomTomTest specs/tomtomtest_auto.lua
local T = TomTomTest
local pass, fail, skipped, na = 0, 0, 0, 0
local failures = {}

for _, test in ipairs(T.tests) do
    local can, reason = true, nil
    if test.needs then can, reason = test.needs() end
    local hasReload = false
    for _, step in ipairs(test.steps) do hasReload = hasReload or step.reload end
    if can == false then
        na = na + 1
    elseif hasReload then
        skipped = skipped + 1
    else
        local ctx = { done = {} }
        T.db.ctx[test.id] = ctx
        local stepError
        for i, step in ipairs(test.steps) do
            ctx.done[i] = true
            if step.run then
                local ok, err = pcall(step.run, ctx, T)
                if not ok then stepError = stepError or ("step " .. i .. ": " .. tostring(err)) end
            end
            Wait(0.3)
        end
        local failed = {}
        if stepError then failed[#failed + 1] = stepError end
        if test.checks then
            local ok, list = pcall(test.checks, ctx, T)
            if not ok then
                failed[#failed + 1] = "checks: " .. tostring(list)
            else
                for _, c in ipairs(list or {}) do
                    if c[2] == false then failed[#failed + 1] = tostring(c[1]) end
                end
            end
        end
        if #failed == 0 then
            pass = pass + 1
        else
            fail = fail + 1
            failures[#failures + 1] = test.id .. " (" .. test.title .. "): " .. table.concat(failed, "; ")
        end
    end
end

print(string.format("TomTomTest: %d passed, %d failed, %d skipped (reload), %d not for this client",
    pass, fail, skipped, na))
for _, line in ipairs(failures) do print("  " .. line) end
