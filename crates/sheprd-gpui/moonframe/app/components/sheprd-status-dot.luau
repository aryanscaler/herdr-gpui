-- What a worktree's or agent's agent is doing, as Sheprd's sidebar shows it:
-- a fixed slot, so labels line up whatever the status, holding a full dot
-- while working, blocked or done, a ring while idle, and a speck when the
-- status is unknown. The colors are Sheprd's own (`theme.status`).

type Props = {
	status: string?,
	theme: any,
	height: number?,
}

local function color(status: string, theme: any)
	local colors = theme.status or {}
	if colors[status] then
		return colors[status]
	elseif status == "working" then
		return theme.warning
	elseif status == "blocked" then
		return theme.danger
	elseif status == "done" then
		return theme.success
	end
	return theme.muted
end

return component {
	title = "Sheprd Status Dot",
	category = "Sheprd",
	icon = "circle",
	description = "An agent's status in a fixed slot: a full dot while working, blocked or done, a ring while idle, a speck when unknown.",
	when = "Before a worktree or agent row's label, so every label starts at the same place.",
	props = {
		["status"] = "\"idle\", \"working\", \"blocked\", \"done\" or \"unknown\"",
		["theme"] = "`app.sheprd_theme`, for the colors",
		["height"] = "the slot's height, the row's line height, so the dot sits on its first line (16)",
	},
	example = { status = "working", theme = {} },

	build = function(props: Props)
		local status = props.status or "unknown"
		local theme = props.theme or {}
		local tint = color(status, theme)
		local size = if status == "unknown" then 3 else 8
		return ui.row {
			class = "status-slot",
			width = 8,
			height = props.height or 16,
			shrink = false,
			align = "center",
			justify = "center",
			ui.row {
				class = "status-dot",
				width = size,
				height = size,
				shrink = false,
				style = if status == "idle"
					then { border = "all", border_color = tint, radius = size / 2 }
					else { background = tint, radius = size / 2 },
			},
		}
	end,
}
