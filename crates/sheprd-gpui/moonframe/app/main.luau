-- The window's layout. Everything here is yours to move, and each part is a
-- mod in mods/: copy one to ~/.config/sheprd-moonframe/mods to change it.
-- Only the terminals and pages in the middle are drawn by Sheprd in Rust.

local function region(name: string)
	return ui.view { name = "sheprd", id = name, props = { region = name } }
end

return mod {
	-- Sheprd's own window opens at 1200 by 780 too.
	window = { size = { width = 1200, height = 780 }, titlebar = "hidden", traffic_lights = { x = 9, y = 9 } },

	init = function(previous)
		-- Sizes come from Sheprd as soon as it starts; `details` is the user's.
		return {
			activity_bar = 48,
			details = previous and previous.details or false,
			header = 34,
			left = 300,
			status_bar = 24,
		}
	end,

	update = function(state, msg)
		if msg == "toggle-details" then
			state.details = not state.details
		elseif type(msg) == "table" and msg.left then
			-- Zero is meant: the activity bar turned off, or Zen mode hiding
			-- the left column, and then neither is drawn.
			state.activity_bar = msg.activity_bar
			state.header = msg.header
			state.left = msg.left
			state.status_bar = msg.status_bar
		end
	end,

	view = function(state, app)
		local theme = app.sheprd_theme or {}
		return ui.column {
			grow = true,
			gap = "none",
			style = { background = theme.background },
			ui.row {
				grow = true,
				gap = "none",
				if state.left > 0 then ui.column {
					width = state.left,
					shrink = false,
					gap = "none",
					ui.row { height = state.header, shrink = false, gap = "none", style = { background = theme.sidebar }, ui.column { grow = true, gap = "none", ui.mount "header" } },
					ui.row {
						grow = true,
						gap = "none",
						if state.activity_bar > 0
							then ui.column { width = state.activity_bar, shrink = false, gap = "none", style = { background = theme.sidebar }, ui.mount "activity" }
							else false,
						-- The left pane: the worktrees, then the agents, each a mod.
						ui.column {
							class = "sheprd-left",
							grow = true,
							gap = "none",
							style = { background = theme.sidebar },
							ui.mount "worktrees",
							ui.mount "agents",
							-- Its edge resizes it, as Sheprd's does; a double click
							-- puts back the default width.
							ui.row {
								class = "sheprd-left-edge",
								float = "right",
								width = 7,
								justify = "end",
								cursor = "ew-resize",
								tooltip = "Drag to resize, double-click to reset",
								pointer = { sidebar_width = true },
								to = "provider:sheprd",
								ui.column { class = "sheprd-left-edge-line", width = 1, grow = true, self_align = "stretch" },
							},
						},
					},
				} else false,
				ui.column {
					grow = true,
					gap = "none",
					ui.hsplit {
						id = "body",
						divider = false,
						ui.pane { ui.column { grow = true, gap = "none", region "main" } },
						ui.pane { size = 300, min = 200, visible = state.details, ui.mount "details" },
					},
					ui.row { height = state.status_bar, shrink = false, gap = "none", style = { background = theme.surface }, ui.column { grow = true, gap = "none", ui.mount "status" } },
				},
			},
			-- A worktree build's strip, across the window's foot.
			ui.mount "build",
			-- The sparkles (the built-in feedback mod): say what to change, and
			-- an agent changes this layout, asking before each edit.
			ui.row {
				class = "feedback-corner",
				float = { at = "bottom-right", inset = 40 },
				ui.mount "feedback",
			},
		}
	end,
}
