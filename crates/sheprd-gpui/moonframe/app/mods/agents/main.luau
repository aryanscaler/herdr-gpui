-- The left pane's agents, under the worktrees (`mods/worktrees`), as Sheprd's
-- agents list draws them in the configured layout: where each agent runs
-- (its workspace, tab and pane) over the agent itself, and its status's word
-- when its integration shows one. Orca and Minimal keep each agent to one
-- line. Rows are their lines, sized by their text; the order is Sheprd's
-- sort (`app.sheprd.agent_sort`), the highlight the focused pane's, and a
-- click shows that pane.
local row = use "sheprd-row"
local status_dot = use "sheprd-status-dot"
local subhead = use "sheprd-subhead"

-- The tallest the list grows before it scrolls, leaving the worktrees room.
local MOST = 280
local LINK = "provider:sheprd"
-- The status dot's slot, which the lines under a row's first step past.
local DOT = 8
-- The agents with a logo of their own; others show a plain mark.
local LOGOS = {
	amp = true,
	claude = true,
	codex = true,
	copilot = true,
	cursor = true,
	droid = true,
	gemini = true,
	grok = true,
	kiro = true,
	opencode = true,
	qwen = true,
}

local function logo(kind: string?): string
	if kind and LOGOS[kind] then
		return `agent-{kind}`
	end
	return "asterisk"
end

-- Where the agent runs, its workspace in the row's weight and the tab and
-- pane beside it; with its workspace gone, the agent names the row.
local function place(agent: any, theme: any)
	local rest = {}
	-- With several devices, the host leads, as Sheprd's rows name it.
	local host = if agent.host then ui.text { `{agent.host} · `, class = "agent-host", style = { color = theme.muted } } else false
	if agent.workspace_label and agent.tab then
		table.insert(rest, agent.tab)
	end
	if agent.pane_name then
		table.insert(rest, agent.pane_name)
	end
	return ui.row {
		class = "agent-place",
		grow = true,
		gap = "none",
		host,
		ui.text { agent.workspace_label or agent.name or "agent", class = "agent-workspace", weight = "bold", truncate = true, style = { color = theme.foreground } },
		if #rest > 0 then ui.text { ` · {table.concat(rest, " · ")}`, class = "agent-pane", truncate = true, style = { color = theme.muted } } else false,
	}
end

local function who(agent: any, theme: any)
	return ui.row {
		class = "agent-who",
		align = "center",
		style = { gap = 4 },
		ui.icon { logo(agent.kind), class = "agent-icon", size = "small", style = { color = theme.muted } },
		ui.text { agent.name or "agent", class = "agent-name", truncate = true, style = { color = theme.muted } },
	}
end

-- A line under the first, starting where the place does.
local function under(gap: number, child: any)
	return ui.row { gap = "none", ui.row { width = DOT + gap, shrink = false }, child }
end

local function agent_row(agent: any, layout: any, theme: any)
	local gap = layout.gap or 6
	local single = layout.name == "orca" or layout.name == "minimal"
	local first
	if single then
		-- The agent's name keeps its room; the workspace gives way first.
		first = ui.row {
			grow = true,
			gap = "none",
			ui.text { agent.workspace_label or agent.name or "agent", class = "agent-workspace", weight = "bold", truncate = true, style = { color = theme.foreground } },
			ui.row { shrink = false, ui.text { ` · {agent.name or "agent"}`, class = "agent-name", style = { color = theme.muted } } },
		}
	else
		first = place(agent, theme)
	end
	return row {
		class = "agent-row",
		selected = agent.focused,
		layout = layout,
		theme = theme,
		msg = { focus_pane = agent.pane, endpoint = agent.endpoint },
		-- Sheprd's own menu for the agent's pane, at the pointer.
		context = { menu = "agent", pane = agent.pane, endpoint = agent.endpoint },
		to = LINK,
		tooltip = agent.title,
		ui.row {
			align = "center",
			style = { gap = gap },
			status_dot { status = agent.status, theme = theme, height = DOT },
			first,
		},
		if single then false else under(gap, who(agent, theme)),
		if not single and agent.state
			then under(gap, ui.text { agent.state, class = "agent-state", truncate = true, style = { color = (theme.status or {})[agent.status] or theme.muted } })
			else false,
	}
end

return mod {
	init = function(previous)
		return previous or {}
	end,

	update = function() end,

	view = function(_, app)
		local sheprd = app.sheprd or { workspaces = {}, agents = {} }
		local theme = app.sheprd_theme or {}
		local layout = sheprd.layout or {}
		-- A page in the sidebar's place, the sidebar collapsed to tiles, or
		-- agents hidden in Sheprd's settings.
		if sheprd.section or sheprd.sidebar_visible == false or sheprd.agents_list == false then
			return ui.column { height = 0 }
		end
		local rows = {}
		for _, agent in sheprd.agents do
			table.insert(rows, agent_row(agent, layout, theme))
		end
		return ui.column {
			class = "sheprd-agents",
			shrink = false,
			gap = "none",
			-- Sheprd's sidebar font: see src/theme.rs.
			style = { background = theme.sidebar, font = "mono" },
			ui.row { class = "sheprd-rule", height = 1, shrink = false, style = { background = theme.active } },
			subhead {
				text = "agents",
				theme = theme,
				layout = layout,
				side = if sheprd.agent_sort
					then ui.row {
						class = "sheprd-link agents-sort",
						msg = { toggle_agent_sort = true },
						to = LINK,
						tooltip = "Sort the agents by place or by what needs you",
						ui.text { sheprd.agent_sort, style = { color = theme.muted } },
					}
					else nil,
			},
			ui.column {
				max_height = MOST,
				shrink = false,
				scroll = true,
				gap = "none",
				ui.column { shrink = false, gap = "none", table.unpack(rows) },
			},
		}
	end,
}
