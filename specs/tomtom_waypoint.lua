-- TomTom: a waypoint north of the player; walk to it and arrive.
-- wowprogramming test --scenario scenarios/dummy --addon TomTom specs/tomtom_waypoint.lua
Command("/way 42 60 Testpoint")
Wait(0.5)
Equal(TomTomCrazyArrow:IsShown(), true, "arrow shown")
Equal(TomTomCrazyArrow.title:GetText(), "Testpoint")
Equal(TomTomCrazyArrow.status:GetText(), "105m away")

-- Walk north for five seconds: 35 yards closer.
Hold("W", 5)
Equal(TomTomCrazyArrow.status:GetText(), "73m away")

-- The rest of the way; TomTom clears the waypoint on arrival.
Hold("W", 12)
Equal(TomTomCrazyArrow:IsShown(), false, "arrow hidden after arriving")
