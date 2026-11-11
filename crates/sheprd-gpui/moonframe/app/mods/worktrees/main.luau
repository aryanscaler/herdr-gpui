-- The left pane's worktrees, drawn in the sidebar layout Sheprd's config
-- names (`layout = "..."`, sent as `app.sheprd.layout` with its spacing):
-- Herdr's own rows at a density, flat or rounded (`normal`, `compact`,
-- `comfortable`, and their `-rounded` forms), Superset's single lines, Orca's
-- cards, or Minimal's status and name. Each row is its lines, sized by their
-- text: the first holds the dot, the name and the badges, centered on each
-- other, and the lines under it start at the name. Inspect reaches each
-- label, branch and pull request. The agents under them are their own mod
-- (`mods/agents`); the highlight is `sheprd-row`'s.
local icon_button = use "icon-button"
local row = use "sheprd-row"
local status_dot = use "sheprd-status-dot"
local subhead = use "sheprd-subhead"

local LINK = "provider:sheprd"
-- The status dot's slot, which the lines under a row's first step past.
local DOT = 8
-- The theme color a status shows in, as `sheprd-status-dot` colors it.
local TONES = { working = "warning", blocked = "danger", done = "success" }

-- Which design draws the rows. Devices lists agents by machine, which this
-- pane does not yet, so it draws Herdr's rows.
local function design(layout: any): string
	local name = layout.name
	if name == "superset" or name == "orca" or name == "minimal" then
		return name
	end
	return "classic"
end

local function label(workspace: any, theme: any)
	local focused = workspace.focused
	return ui.row {
		grow = true,
		ui.text {
			workspace.label,
			class = "worktree-label",
			truncate = true,
			weight = if focused then "bold" else "normal",
			style = { color = if focused then theme.foreground else theme.subtext },
		},
	}
end

-- The branch, when it says something the label does not.
local function branch(workspace: any): string?
	if workspace.branch and workspace.branch ~= workspace.label then
		return workspace.branch
	end
	return nil
end

-- What a click on a row sends: show its workspace, on its device.
local function focus(workspace: any)
	return { focus_workspace = workspace.id, endpoint = workspace.endpoint }
end

-- What a right click sends: Sheprd's own popover for the workspace, with its
-- details, actions and pull request, at the pointer.
local function menu(workspace: any)
	return { menu = "workspace", workspace = workspace.id, endpoint = workspace.endpoint }
end

-- The drag in progress, set by `view` before it builds the rows: whether rows
-- of this device may be dragged (the selected device's, while connected, as
-- Sheprd's own), and where the dragged one would land.
local drag = { device = nil, drop = nil }

-- What a row carries when dragged onto another: its workspace.
local function carry(workspace: any)
	if drag.device ~= workspace.endpoint then
		return nil
	end
	return { workspace = workspace.id }
end

-- What a row tells this mod as a dragged row moves over it and is dropped.
local function accept(workspace: any)
	if drag.device ~= workspace.endpoint then
		return nil
	end
	return { reorder = workspace.id }
end

-- Where the dragged row would land on this one: "before", "after" or nil.
local function drop_at(workspace: any): string?
	local drop = drag.drop
	if drop and drop.target == workspace.id then
		return if drop.after then "after" else "before"
	end
	return nil
end

-- Commits to push and to pull, as Sheprd's rows count them.
local function drift(workspace: any, theme: any)
	local parts = {}
	if (workspace.ahead or 0) > 0 then
		table.insert(parts, `↑{workspace.ahead}`)
	end
	if (workspace.behind or 0) > 0 then
		table.insert(parts, `↓{workspace.behind}`)
	end
	if #parts == 0 then
		return false
	end
	return ui.text { table.concat(parts, " "), class = "worktree-drift", tooltip = "Commits to push ↑ and to pull ↓", style = { color = theme.subtext } }
end

-- The chevron that folds a repository's linked worktrees under its checkout.
local function fold(workspace: any, theme: any)
	if not workspace.group then
		return false
	end
	return ui.row {
		class = "worktree-fold",
		shrink = false,
		msg = { toggle_repository = workspace.group, endpoint = workspace.endpoint },
		to = LINK,
		tooltip = if workspace.collapsed then "Show its worktrees" else "Hide its worktrees",
		ui.icon { if workspace.collapsed then "chevron-right" else "chevron-down", size = "small", style = { color = theme.muted } },
	}
end

-- The note kept on a checkout, under its row; a click edits it.
local function note_line(workspace: any, theme: any)
	return ui.row {
		class = "worktree-note",
		align = "center",
		msg = { note = workspace.id, endpoint = workspace.endpoint },
		to = LINK,
		tooltip = "Edit the note",
		style = { gap = 4 },
		ui.icon { "file-text", size = "small", style = { color = theme.accent } },
		ui.text { workspace.note, truncate = true, style = { color = theme.subtext } },
	}
end

-- What sits at the end of a row, as Sheprd's badge draws it: the muted and
-- teleported marks, the uncommitted-work chip, the commits to push and pull,
-- the pull request's number in its state's color, and the fold chevron, with
-- the pull request's added and removed lines under them when `counts` shows
-- them.
local function badges(workspace: any, counts: boolean, theme: any)
	local dirty = theme.dirty or theme.warning
	return ui.column {
		class = "worktree-badges",
		align = "end",
		gap = "none",
		shrink = false,
		ui.row {
			align = "center",
			style = { gap = 6 },
			if workspace.muted then ui.icon { "bell-off", class = "worktree-muted", size = "small", tooltip = "Notifications muted", style = { color = theme.muted } } else false,
			if workspace.teleported then ui.icon { "send", class = "worktree-teleported", size = "small", tooltip = "Uncommitted work teleported", style = { color = theme.accent } } else false,
			if workspace.dirty
				then ui.row {
					class = "worktree-dirty",
					width = 14,
					height = 14,
					align = "center",
					justify = "center",
					style = { background = `{dirty}30`, border = "all", border_color = `{dirty}90`, radius = 3 },
					ui.icon { "pencil", size = "small", style = { color = theme.dirty_ink or dirty } },
				}
				else false,
			drift(workspace, theme),
			if workspace.pr then ui.text { workspace.pr, class = "worktree-pr", style = { color = workspace.pr_color or theme.muted } } else false,
			fold(workspace, theme),
		},
		if counts and (workspace.added or workspace.removed)
			then ui.row {
				class = "worktree-counts",
				gap = "none",
				ui.text { workspace.added or "", style = { color = theme.added or theme.success } },
				ui.text { "/", style = { color = theme.muted } },
				ui.text { workspace.removed or "", style = { color = theme.removed or theme.danger } },
			}
			else false,
	}
end

-- A line under the first, starting where the name does.
local function under(lead: any, gap: number, child: any)
	return ui.row {
		gap = "none",
		lead or false,
		ui.row { width = DOT + gap, shrink = false },
		child,
	}
end

-- The tree guide a linked worktree hangs from, in the indent before its dot,
-- as tall as the line it is on: the trunk, under its checkout's dot, is the
-- left border of its halves and runs on past every child but the last; on
-- the first line the top half ends in the elbow that reaches to the dot.
local function guide(layout: any, first: boolean, last: boolean, theme: any)
	local width = (layout.child_indent or 20) - (layout.gap or 6)
	local trunk = { border = "left", border_color = theme.active }
	return ui.row {
		class = "worktree-guide",
		width = width,
		shrink = false,
		self_align = "stretch",
		gap = "none",
		ui.row { width = 3, shrink = false },
		ui.column {
			grow = true,
			self_align = "stretch",
			gap = "none",
			ui.column {
				grow = true,
				gap = "none",
				justify = "end",
				style = if first or not last then trunk else nil,
				if first then ui.row { class = "worktree-elbow", height = 1, shrink = false, style = { background = theme.active, margin = { right = 2 } } } else false,
			},
			ui.column { grow = true, style = if not last then trunk else nil },
		},
	}
end

-- Herdr's rows: a dot, the name over its branch, worktrees stepped in under
-- their checkout on tree guides.
local function classic_row(workspace: any, last: boolean, layout: any, theme: any)
	local child = workspace.child
	local gap = layout.gap or 6
	local details = if child then layout.child_details else layout.workspace_details
	local second = if details then branch(workspace) else nil
	local guided = child and layout.tree_lines
	return row {
		class = if child then "worktree-row worktree-child" else "worktree-row",
		selected = workspace.focused,
		layout = layout,
		theme = theme,
		-- With guides, the indent is the guide's, inside the row.
		indent = if child and not guided then layout.child_indent else 0,
		msg = { sheprd = focus(workspace) },
		context = { sheprd = menu(workspace) },
		carry = carry(workspace),
		accept = accept(workspace),
		drop = drop_at(workspace),
		ui.row {
			align = "center",
			style = { gap = gap },
			if guided then guide(layout, true, last and second == nil, theme) else false,
			status_dot { status = workspace.agent_status, theme = theme, height = DOT },
			label(workspace, theme),
			badges(workspace, layout.pr_counts, theme),
		},
		if second
			then under(
				if guided then guide(layout, false, last, theme) else nil,
				if guided then 2 * gap else gap,
				ui.text {
					second,
					class = "worktree-branch",
					truncate = true,
					style = { color = if workspace.focused then theme.accent else theme.muted },
				}
			)
			else false,
		if workspace.note
			then under(
				if guided then guide(layout, false, last, theme) else nil,
				if guided then 2 * gap else gap,
				note_line(workspace, theme)
			)
			else false,
	}
end

-- Superset's rows: one line, an icon slot that reports the row's state, the
-- name, and the pull request's change counts on the right.
local function superset_row(workspace: any, layout: any, theme: any)
	local quiet = workspace.agent_status == nil or workspace.agent_status == "idle" or workspace.agent_status == "unknown"
	return row {
		class = "worktree-row",
		selected = workspace.focused,
		stripe = true,
		layout = { padding = layout.padding, row_padding = 5 },
		theme = theme,
		indent = if workspace.child then layout.padding else 0,
		msg = { sheprd = focus(workspace) },
		context = { sheprd = menu(workspace) },
		carry = carry(workspace),
		accept = accept(workspace),
		drop = drop_at(workspace),
		ui.row {
			align = "center",
			style = { gap = 8 },
			ui.row {
				width = 14,
				shrink = false,
				justify = "center",
				if quiet
					then ui.icon { if workspace.child then "git-branch" else "folder-git-2", class = "worktree-icon", size = "small", style = { color = theme.muted } }
					else status_dot { status = workspace.agent_status, theme = theme, height = DOT },
			},
			label(workspace, theme),
			badges(workspace, true, theme),
		},
	}
end

-- `workspace` without its pull request, for a badge whose row names it
-- elsewhere.
local function without_pr(workspace: any)
	local copy = table.clone(workspace)
	copy.pr = nil
	return copy
end

-- Orca's cards: a status column beside the name, and a quieter meta line
-- with the branch and the pull request.
local function orca_row(workspace: any, theme: any)
	local meta = {}
	if branch(workspace) then
		table.insert(meta, workspace.branch)
	end
	if workspace.pr then
		table.insert(meta, workspace.pr)
	end
	return row {
		class = "worktree-row worktree-card",
		selected = workspace.focused,
		card = true,
		layout = { padding = 8, row_padding = 5, spacing = 4 },
		theme = theme,
		indent = if workspace.child then 12 else 0,
		msg = { sheprd = focus(workspace) },
		context = { sheprd = menu(workspace) },
		carry = carry(workspace),
		accept = accept(workspace),
		drop = drop_at(workspace),
		ui.row {
			align = "center",
			style = { gap = 6 },
			ui.row { width = 12, shrink = false, justify = "center", status_dot { status = workspace.agent_status, theme = theme, height = DOT } },
			label(workspace, theme),
			badges(without_pr(workspace), false, theme),
		},
		if #meta > 0
			then ui.row {
				gap = "none",
				ui.row { width = 18, shrink = false },
				ui.text { table.concat(meta, " · "), class = "worktree-meta", truncate = true, size = "small", style = { color = theme.muted } },
			}
			else false,
	}
end

-- Minimal's rows: the status and the name, on one line.
local function minimal_row(workspace: any, layout: any, theme: any)
	return row {
		class = "worktree-row",
		selected = workspace.focused,
		layout = layout,
		theme = theme,
		indent = if workspace.child then layout.child_indent else 0,
		msg = { sheprd = focus(workspace) },
		context = { sheprd = menu(workspace) },
		carry = carry(workspace),
		accept = accept(workspace),
		drop = drop_at(workspace),
		ui.row {
			align = "center",
			style = { gap = layout.gap or 6 },
			status_dot { status = workspace.agent_status, theme = theme, height = DOT },
			label(workspace, theme),
			fold(workspace, theme),
		},
	}
end

-- A row's note, under it, for the layouts whose rows draw no lines of their
-- own beneath the name.
local function with_note(row_element: any, workspace: any, theme: any)
	if not workspace.note then
		return row_element
	end
	return ui.column {
		gap = "none",
		row_element,
		ui.row { gap = "none", ui.row { width = DOT + 20, shrink = false }, note_line(workspace, theme) },
	}
end

local function workspace_row(workspace: any, last: boolean, layout: any, theme: any)
	local kind = design(layout)
	if kind == "superset" then
		return with_note(superset_row(workspace, layout, theme), workspace, theme)
	elseif kind == "orca" then
		return with_note(orca_row(workspace, theme), workspace, theme)
	elseif kind == "minimal" then
		return with_note(minimal_row(workspace, layout, theme), workspace, theme)
	end
	return classic_row(workspace, last, layout, theme)
end


-- A row when the sidebar is collapsed: its number on a tile, the label in the
-- tooltip.
local function compact_row(workspace: any, theme: any)
	return ui.column {
		class = "worktree-tile",
		selected = workspace.focused,
		align = "center",
		justify = "center",
		height = 36,
		shrink = false,
		msg = focus(workspace),
		context = menu(workspace),
		to = LINK,
		tooltip = workspace.label,
		ui.text {
			`{workspace.number}`,
			class = "worktree-number",
			weight = if workspace.focused then "bold" else "normal",
			style = { color = TONES[workspace.agent_status] and theme[TONES[workspace.agent_status]] or theme.muted },
		},
	}
end

-- A device's header, as Sheprd lists several: its fold arrow, its name, its
-- machine's load, and whether it is connected. A click selects it, a right
-- click opens its menu.
local function device_header(device: any, layout: any, theme: any)
	local load = device.load
	return ui.row {
		class = "sheprd-device",
		selected = device.selected,
		align = "center",
		shrink = false,
		msg = { select_device = device.id },
		context = { menu = "device", endpoint = device.id },
		to = LINK,
		style = {
			gap = 6,
			padding = { x = (layout.padding or 8) + (layout.inset or 0), y = 3 },
			background = if device.selected then theme.active else nil,
			opacity = if device.enabled == false then 0.6 else nil,
		},
		ui.row {
			class = "sheprd-device-fold",
			shrink = false,
			msg = { toggle_device = device.id },
			to = LINK,
			tooltip = if device.collapsed then "Show its workspaces" else "Hide its workspaces",
			ui.icon { if device.collapsed then "chevron-right" else "chevron-down", size = "small", style = { color = theme.muted } },
		},
		ui.icon { if device.id == "local" then "laptop" else "server", size = "small", style = { color = theme.subtext } },
		ui.row { grow = true, ui.text { device.label, class = "sheprd-device-label", truncate = true, weight = "bold", style = { color = theme.foreground } } },
		if load and load.cpu
			then ui.text { `{math.floor(load.cpu + 0.5)}%`, class = "sheprd-device-cpu", tooltip = "CPU", style = { color = theme.muted } }
			else false,
		if load and load.memory
			then ui.text { `{math.floor(load.memory + 0.5)}%`, class = "sheprd-device-memory", tooltip = "Memory", style = { color = theme.muted } }
			else false,
		ui.row {
			class = "sheprd-device-status",
			width = 7,
			height = 7,
			shrink = false,
			tooltip = if device.connected then "Connected" else "Not connected",
			style = { radius = 4, background = if device.connected then theme.success else theme.muted },
		},
	}
end

-- Whether any repository on any device has its worktrees folded away.
local function any_folded(devices: any): boolean
	for _, device in devices do
		for _, workspace in device.workspaces or {} do
			if workspace.collapsed then
				return true
			end
		end
	end
	return false
end

-- Search, as Sheprd's sidebar searches: a case-insensitive substring of
-- device names, worktree names, branches and notes, over every workspace,
-- folded or not, as flat results grouped by what matched.
local MOST_HITS = 200
local SECTIONS = { "devices", "worktrees", "branches", "notes" }

-- The results for `query`, by section, each with what opening it sends.
local function search(devices: any, query: string)
	local needle = string.lower(query)
	local found = { devices = {}, worktrees = {}, branches = {}, notes = {} }
	local count = 0
	local function add(section: string, hit: any)
		if count < MOST_HITS then
			count += 1
			table.insert(found[section], hit)
		end
	end
	local function has(text: string?): boolean
		return text ~= nil and string.find(string.lower(text), needle, 1, true) ~= nil
	end
	for _, device in devices do
		if has(device.label) then
			add("devices", { text = device.label, icon = "laptop", open = { select_device = device.id } })
		end
		for _, workspace in device.search or {} do
			local open = { focus_workspace = workspace.id, endpoint = workspace.endpoint }
			if has(workspace.label) then
				add("worktrees", { text = workspace.label, status = workspace.agent_status, open = open })
			end
			if has(workspace.branch) then
				add("branches", { text = workspace.branch, status = workspace.agent_status, open = open })
			end
			if has(workspace.note) then
				add("notes", { text = workspace.note, icon = "file-text", open = open })
			end
		end
	end
	local flat = {}
	for _, section in SECTIONS do
		for _, hit in found[section] do
			table.insert(flat, hit)
		end
	end
	return found, flat
end

-- The hits the last view showed, in order, for Enter to open.
local shown = {}

local function hit_row(hit: any, selected: boolean, layout: any, theme: any)
	return row {
		class = "worktree-hit",
		selected = selected,
		layout = layout,
		theme = theme,
		msg = { sheprd = hit.open, close_search = true },
		ui.row {
			align = "center",
			style = { gap = layout.gap or 6 },
			if hit.icon
				then ui.icon { hit.icon, size = "small", style = { color = theme.muted } }
				else status_dot { status = hit.status, theme = theme, height = DOT },
			ui.row { grow = true, ui.text { hit.text, truncate = true, style = { color = theme.foreground } } },
		},
	}
end

local function results(devices: any, query: string, selected: number, layout: any, theme: any)
	local found, flat = search(devices, query)
	shown = flat
	local children = {}
	local index = 0
	for _, section in SECTIONS do
		if #found[section] > 0 then
			table.insert(children, subhead { text = section, theme = theme, layout = layout })
			for _, hit in found[section] do
				index += 1
				table.insert(children, hit_row(hit, index == selected, layout, theme))
			end
		end
	end
	if #children == 0 then
		table.insert(children, ui.row {
			style = { padding = { x = (layout.padding or 8) + (layout.inset or 0), y = 6 } },
			ui.text { "No matches", style = { color = theme.muted } },
		})
	end
	return children
end

local function link(text: string, command: string, theme: any)
	return ui.row {
		class = "sheprd-link",
		msg = { command = command },
		to = LINK,
		ui.text { text, style = { color = theme.muted } },
	}
end

return mod {
	init = function(previous)
		return previous or { query = "", searching = false, selected = 1 }
	end,

	update = function(state, msg)
		if type(msg) == "table" and msg.sheprd then
			-- A row's click or right click, for Sheprd, and opening a hit
			-- closes the search.
			if msg.close_search then
				state.searching = false
				state.query = ""
			end
			return state, { { to = LINK, msg = msg.sheprd } }
		elseif type(msg) == "table" and msg.reorder then
			-- A dragged row over, off, or dropped on the row `msg.reorder`.
			local moving = msg.carry and msg.carry.workspace
			if msg.phase == "leave" then
				if state.drop and state.drop.target == msg.reorder then
					state.drop = nil
				end
			elseif moving and moving ~= msg.reorder then
				local after = (msg.y or 0) > 0.5
				if msg.phase == "drop" then
					state.drop = nil
					return state, { { to = LINK, msg = { move_workspace = moving, target = msg.reorder, after = after } } }
				end
				state.drop = { target = msg.reorder, after = after }
			end
		elseif msg == "search" then
			state.searching = not state.searching
			state.query = ""
			state.selected = 1
		elseif msg == "search_close" then
			state.searching = false
			state.query = ""
		elseif type(msg) == "table" and msg.query then
			state.query = msg.value or ""
			state.selected = 1
		elseif msg == "search_next" then
			state.selected = math.min(state.selected + 1, math.max(#shown, 1))
		elseif msg == "search_previous" then
			state.selected = math.max(state.selected - 1, 1)
		elseif msg == "search_open" then
			local hit = shown[state.selected]
			if hit then
				state.searching = false
				state.query = ""
				return state, { { to = LINK, msg = hit.open } }
			end
		end
		return state
	end,

	view = function(state, app)
		local sheprd = app.sheprd or { workspaces = {}, devices = {} }
		local devices = sheprd.devices or {}
		local theme = app.sheprd_theme or {}
		local layout = sheprd.layout or {}
		-- Only the selected device's rows move, while it is connected, as in
		-- Sheprd's own sidebar.
		drag.device = nil
		for _, device in devices do
			if device.selected and device.connected then
				drag.device = device.id
			end
		end
		drag.drop = state.drop
		if sheprd.section then
			return ui.column { width = 0 }
		end
		if sheprd.sidebar_visible == false then
			local tiles = {}
			for _, device in devices do
				for _, workspace in device.workspaces or {} do
					table.insert(tiles, compact_row(workspace, theme))
				end
			end
			return ui.column {
				class = "sheprd-worktrees",
				grow = true,
				scroll = true,
				padding = "small",
				style = { background = theme.sidebar, gap = 4 },
				table.unpack(tiles),
			}
		end
		local searching = state.searching and state.query ~= ""
		local rows = {}
		if searching then
			rows = results(devices, state.query, state.selected, layout, theme)
		else
			-- Each device under its header when several are listed, as
			-- Sheprd's sidebar shows them; one device's rows stand alone.
			for _, device in devices do
				if sheprd.multi then
					table.insert(rows, device_header(device, layout, theme))
				end
				local list = device.workspaces or {}
				for index, workspace in list do
					local following = list[index + 1]
					table.insert(rows, workspace_row(workspace, not (following and following.child), layout, theme))
				end
			end
		end
		local folded = any_folded(devices)
		local head = if state.searching
			then ui.row {
				class = "sheprd-search",
				align = "center",
				shrink = false,
				style = { padding = { x = (layout.padding or 8) + (layout.inset or 0), y = 4 } },
				ui.input {
					id = "sidebar-search",
					class = "sheprd-search-input",
					grow = true,
					focus = true,
					placeholder = "Search devices, worktrees, branches, notes",
					change = { query = true },
					submit = "search_open",
					keys = { escape = "search_close", up = "search_previous", down = "search_next" },
				},
				icon_button { icon = "circle-x", size = "small", tooltip = "Close the search", msg = "search_close" },
			}
			else subhead {
				text = "spaces",
				theme = theme,
				layout = layout,
				side = ui.row {
					align = "center",
					style = { gap = 2 },
					icon_button {
						icon = if folded then "chevrons-up-down" else "list-tree",
						size = "small",
						tooltip = if folded then "Expand every group" else "Collapse every group",
						msg = { fold_all = not folded },
						to = LINK,
					},
					icon_button { icon = "search", size = "small", tooltip = "Search devices, worktrees, branches and notes", msg = "search" },
				},
			}
		return ui.column {
			class = "sheprd-worktrees",
			grow = true,
			gap = "none",
			-- Sheprd's sidebar font: see src/theme.rs.
			style = { background = theme.sidebar, font = "mono" },
			ui.row {
				class = "sheprd-head",
				align = "center",
				height = 32,
				shrink = false,
				style = { padding = { left = 12, right = 8 } },
				ui.text { "WORKSPACES", class = "sheprd-title", weight = "bold", size = "small", style = { color = theme.subtext } },
				ui.spacer(),
				icon_button { icon = "panel-left", size = "small", tooltip = "Toggle the sidebar", msg = { command = "toggle_sidebar" }, to = LINK },
			},
			head,
			ui.column {
				grow = true,
				scroll = true,
				gap = "none",
				ui.column { shrink = false, gap = "none", table.unpack(rows) },
			},
			ui.row {
				class = "sheprd-left-actions",
				align = "center",
				shrink = false,
				style = { padding = { x = (layout.padding or 8) + (layout.inset or 0), y = 4 } },
				link("new", "new_workspace", theme),
				ui.spacer(),
				link("menu", "command_palette", theme),
			},
		}
	end,
}
