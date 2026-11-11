-- The status bar: the connection, then the machine's load and quick commands.
local meter = use "meter"

local function run(command: string)
	return { command = command }
end

local function item(icon: string, text: string, color: string, msg: any, to: string?)
	return ui.row {
		class = "status-item",
		align = "center",
		shrink = false,
		msg = msg,
		to = to,
		style = { gap = 5 },
		ui.icon { icon, size = "small", style = { color = color } },
		ui.text { text, size = "small", style = { color = color } },
	}
end

return mod {
	init = function(previous)
		return previous or {}
	end,

	update = function() end,

	view = function(_, app)
		local sheprd = app.sheprd or { agents = {} }
		local load = app.sheprd_load or {}
		local working = 0
		for _, agent in sheprd.agents do
			if agent.status == "working" then
				working += 1
			end
		end
		local theme = app.sheprd_theme or {}
		return ui.row {
			class = "sheprd-status",
			grow = true,
			align = "center",
			style = { background = theme.surface, gap = 14, padding = { x = 12 } },
			if sheprd.connected == false then item("circle-alert", sheprd.status, theme.danger) else false,
			if working > 0 then item("bot", `{working} working`, theme.accent) else false,
			ui.spacer(),
			if load.cpu then meter { label = "CPU", value = load.cpu / 100, width = 40, warn = 0.8 } else false,
			if load.memory then meter { label = "MEM", value = load.memory / 100, width = 40, warn = 0.8 } else false,
			item("panel-right", "Details", theme.subtext, "toggle-details", "shell"),
			item("palette", "Theme", theme.subtext, run "themes", "provider:sheprd"),
			item("command", "Commands", theme.subtext, run "command_palette", "provider:sheprd"),
		}
	end,
}
