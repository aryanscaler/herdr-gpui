-- The right pane: the focused worktree and the agents in it. Open it from
-- Details in the status bar.
return mod {
	init = function(previous)
		return previous or {}
	end,

	update = function() end,

	view = function(_, app)
		local sheprd = app.sheprd or { workspaces = {}, agents = {} }
		local theme = app.sheprd_theme or {}
		local focused
		for _, workspace in sheprd.workspaces do
			if workspace.focused then
				focused = workspace
			end
		end
		if not focused then
			return ui.column { class = "sheprd-right", grow = true, padding = "normal", ui.text { "No worktree in focus", tone = "muted" } }
		end
		local rows = {}
		for _, agent in sheprd.agents do
			-- Agents come from every listed device; the focused worktree is the
			-- selected device's, and another may use the same workspace id.
			if agent.workspace == focused.id and agent.endpoint == focused.endpoint then
				table.insert(rows, ui.row {
					class = "details-agent",
					align = "center",
					gap = "small",
					msg = { focus_pane = agent.pane, endpoint = agent.endpoint },
					to = "provider:sheprd",
					ui.text { agent.name or "agent", truncate = true },
					ui.spacer(),
					ui.badge { agent.status },
				})
			end
		end
		return ui.column {
			class = "sheprd-right",
			grow = true,
			gap = "normal",
			padding = "normal",
			style = { background = theme.sidebar },
			ui.text { focused.label, class = "details-title", size = "large", weight = "bold", truncate = true },
			ui.text { focused.branch or "No branch", class = "details-branch", tone = "muted", truncate = true },
			ui.row {
				class = "details-counts",
				gap = "small",
				ui.badge { `↑{focused.ahead}` },
				ui.badge { `↓{focused.behind}` },
				if focused.pr then ui.badge { focused.pr } else false,
			},
			ui.divider(),
			ui.text { "Agents", weight = "semibold" },
			if #rows > 0 then ui.column { gap = "small", table.unpack(rows) } else ui.text { "None here", tone = "muted" },
		}
	end,
}
