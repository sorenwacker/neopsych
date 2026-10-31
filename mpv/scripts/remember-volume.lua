-- Keeps mute and volume as they were last left, across restarts.
local path = mp.command_native({ "expand-path", "~~/volume.txt" })

local f = io.open(path, "r")
if f then
    local mute, volume = f:read("*l"), f:read("*n")
    f:close()
    mp.set_property_bool("mute", mute == "yes")
    if volume then
        mp.set_property_number("volume", volume)
    end
end

local function save()
    local mute, volume = mp.get_property("mute"), mp.get_property_number("volume")
    if not mute or not volume then
        return
    end
    local out = io.open(path, "w")
    if out then
        out:write(mute, "\n", volume, "\n")
        out:close()
    end
end

mp.observe_property("mute", "string", save)
mp.observe_property("volume", "number", save)
