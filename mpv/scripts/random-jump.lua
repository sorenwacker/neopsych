-- Random mode: every 10 to 40 seconds, jump to a random other video in the
-- same folder, starting somewhere in its middle. N switches it, Shift+N jumps now.
local utils = require("mp.utils")

local VIDEO = { mp4 = true, mkv = true, webm = true, mov = true, avi = true, m4v = true, wmv = true }
local MIN_SECONDS, MAX_SECONDS = 10, 40

local on = false
local timer = nil
local was_keep_open = nil

local function videos()
    local path = mp.get_property("path")
    if not path then
        return {}, nil
    end
    local dir = utils.split_path(path)
    local list = {}
    for _, name in ipairs(utils.readdir(dir, "files") or {}) do
        local ext = name:match("%.([^%.]+)$")
        if ext and VIDEO[ext:lower()] then
            list[#list + 1] = utils.join_path(dir, name)
        end
    end
    return list, path
end

local function jump()
    local list, current = videos()
    local choices = {}
    for _, p in ipairs(list) do
        if p ~= current then
            choices[#choices + 1] = p
        end
    end
    if #choices == 0 then
        mp.osd_message("Random mode: no other videos in this folder")
        return
    end
    local pick = choices[math.random(#choices)]
    mp.commandv("loadfile", pick, "replace", "-1", "start=" .. math.random(5, 80) .. "%")
end

local function schedule()
    if timer then
        timer:kill()
    end
    if on then
        timer = mp.add_timeout(MIN_SECONDS + math.random() * (MAX_SECONDS - MIN_SECONDS), jump)
    end
end

-- Each new video gets its own random time before the next jump.
mp.register_event("file-loaded", schedule)

-- A video that ends in random mode jumps on instead of closing mpv.
mp.observe_property("eof-reached", "bool", function(_, eof)
    if on and eof then
        jump()
    end
end)

mp.add_key_binding(nil, "toggle", function()
    on = not on
    if on then
        was_keep_open = mp.get_property("keep-open")
        mp.set_property("keep-open", "yes")
    elseif was_keep_open then
        mp.set_property("keep-open", was_keep_open)
    end
    schedule()
    mp.osd_message("Random mode: " .. (on and "on" or "off"))
end)

mp.add_key_binding(nil, "next", jump)

math.randomseed(os.time())

-- --script-opts=random_jump-on=yes starts with random mode on.
local opts = { on = false }
require("mp.options").read_options(opts)
if opts.on then
    on = true
    mp.set_property("keep-open", "yes")
end
