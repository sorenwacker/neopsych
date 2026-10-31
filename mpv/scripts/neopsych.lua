-- Hotkeys for the neopsych video effects (see input.conf): each one says on
-- screen what it switched to, and stepping one shader setting keeps the others.

local SHADERS = {
    colour = "~~/shaders/neopsych.glsl",
    kaleido = "~~/shaders/kaleido.glsl",
}
local BLUR = "@blur:tmix=frames=6:weights=1 2 3 4 5 6"

local function onoff(b)
    return b and "on" or "off"
end

local function shader_on(path)
    local want = mp.command_native({ "expand-path", path })
    for _, s in ipairs(mp.get_property_native("glsl-shaders", {})) do
        if mp.command_native({ "expand-path", s }) == want then
            return true
        end
    end
    return false
end

local function blur_on()
    for _, f in ipairs(mp.get_property_native("vf", {})) do
        if f.label == "blur" then
            return true
        end
    end
    return false
end

-- Each setting steps through its values, starting from the shader's default.
local steps = {
    speed = { label = "Colour speed", values = { 0.12, 0.4, 1.0, 0.04 }, i = 1 },
    amount = { label = "Effect strength", values = { 1, 0.5, 0.25 }, i = 1 },
    sectors = { label = "Kaleidoscope wedges", values = { 3, 2, 4, 6 }, i = 1 },
}

-- Everything the keys switch is kept in effects.txt next to this folder, so
-- mpv starts the way it was last left.
local STATE_FILE = mp.command_native({ "expand-path", "~~/effects.txt" })

local function save_state()
    local f = io.open(STATE_FILE, "w")
    if not f then
        return
    end
    f:write("colour=", onoff(shader_on(SHADERS.colour)), "\n")
    f:write("kaleido=", onoff(shader_on(SHADERS.kaleido)), "\n")
    f:write("blur=", onoff(blur_on()), "\n")
    for key, s in pairs(steps) do
        f:write(key, "=", s.i, "\n")
    end
    f:close()
end

local function toggle_shader(name, label)
    mp.commandv("change-list", "glsl-shaders", "toggle", SHADERS[name])
    mp.osd_message(label .. ": " .. onoff(shader_on(SHADERS[name])))
    save_state()
end

for key, s in pairs(steps) do
    mp.add_key_binding(nil, key, function()
        s.i = s.i % #s.values + 1
        local v = s.values[s.i]
        mp.commandv("change-list", "glsl-shader-opts", "append", key .. "=" .. v)
        mp.osd_message(s.label .. ": " .. v)
        save_state()
    end)
end

mp.add_key_binding(nil, "colour", function()
    toggle_shader("colour", "Colour effect")
end)
-- Slow motion: below normal speed, fade from frame to frame on every screen
-- refresh instead of holding each frame, so slowed video glides instead of
-- stepping. At normal speed mpv plays as it always does.
local smooth = false

-- The shaders' clock is frames drawn / rate + offset. mpv draws a frame on
-- every screen refresh while blending, else once per video frame. When that
-- rate changes, the offset is moved so the clock carries on without a jump;
-- the frame count it needs is estimated from the time spent drawing.
local rate, offset = 60, 0
local frames, since, paused = 0, mp.get_time(), false

local function count_frames()
    local now = mp.get_time()
    if not paused then
        frames = frames + rate * (now - since)
    end
    since = now
end

local function update_rate()
    count_frames()
    local new
    if smooth then
        new = mp.get_property_number("display-fps", 60)
    else
        new = mp.get_property_number("container-fps", 30) * mp.get_property_number("speed", 1)
    end
    if math.abs(new - rate) < 0.01 then
        return
    end
    offset = offset + frames / rate - frames / new
    rate = new
    mp.commandv("change-list", "glsl-shader-opts", "append", string.format("rate=%.4f", rate))
    mp.commandv("change-list", "glsl-shader-opts", "append", string.format("offset=%.4f", offset))
end

-- mpv restarts its frame counter for every new file (not on seeks), so the
-- time counted so far moves into the offset and the effects carry on.
mp.register_event("file-loaded", function()
    count_frames()
    offset = offset + frames / rate
    frames = 0
    mp.commandv("change-list", "glsl-shader-opts", "append", string.format("offset=%.4f", offset))
end)

-- Nothing is drawn while paused, so those seconds don't count.
mp.observe_property("pause", "bool", function(_, p)
    count_frames()
    paused = p
end)

mp.observe_property("speed", "number", function(_, speed)
    local slow = (speed or 1) < 0.99
    if slow ~= smooth then
        smooth = slow
        mp.set_property("video-sync", slow and "display-resample" or "audio")
        mp.set_property("interpolation", slow and "yes" or "no")
        mp.set_property("tscale", "linear")
        if slow then
            mp.osd_message(string.format("Speed: %.2f, smooth slow motion", speed))
        end
    end
    update_rate()
end)
mp.observe_property("container-fps", "number", update_rate)
mp.observe_property("display-fps", "number", update_rate)

-- The kaleidoscope draws a 16:9 picture whatever the video's shape, so mpv
-- must show it as 16:9 while it is on.
local function fit_kaleido()
    local on = shader_on(SHADERS.kaleido)
    mp.set_property("video-aspect-override", on and "16:9" or "no")
end

mp.add_key_binding(nil, "kaleido", function()
    toggle_shader("kaleido", "Kaleidoscope")
    fit_kaleido()
end)
-- Also when it was switched on from the command line.
mp.register_event("file-loaded", fit_kaleido)
mp.add_key_binding(nil, "blur", function()
    mp.commandv("vf", "toggle", BLUR)
    mp.osd_message("Motion blur: " .. onoff(blur_on()))
    save_state()
end)

-- Start the way it was last left (see save_state).
local f = io.open(STATE_FILE, "r")
if f then
    local saved = {}
    for line in f:lines() do
        local k, v = line:match("^(%w+)=(%w+)$")
        if k then
            saved[k] = v
        end
    end
    f:close()
    -- Colour before kaleidoscope, the order the keys usually give.
    for _, name in ipairs({ "colour", "kaleido" }) do
        local path = SHADERS[name]
        if (saved[name] == "on") ~= shader_on(path) then
            mp.commandv("change-list", "glsl-shaders", "toggle", path)
        end
    end
    if (saved.blur == "on") ~= blur_on() then
        mp.commandv("vf", "toggle", BLUR)
    end
    for key, s in pairs(steps) do
        local i = tonumber(saved[key])
        if i and s.values[i] then
            s.i = i
            mp.commandv("change-list", "glsl-shader-opts", "append", key .. "=" .. s.values[i])
        end
    end
end
