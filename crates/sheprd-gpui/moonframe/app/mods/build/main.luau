-- The yellow strip at the window's foot of a build made from a worktree, as
-- Sheprd's own window hangs it: the branch the build came from, and its pull
-- request, which opens in the browser. Other builds draw nothing here.
local LINK = "provider:sheprd"

return mod {
	init = function(previous)
		return previous or {}
	end,

	update = function() end,

	view = function(_, app)
		local build = (app.sheprd or {}).build or {}
		if not build.worktree then
			return ui.row { height = 0 }
		end
		return ui.row {
			class = "sheprd-build",
			align = "center",
			height = 22,
			shrink = false,
			style = { background = "#f6c453", padding = { x = 12 }, gap = 10 },
			ui.row { grow = true, ui.text { build.branch or "", class = "sheprd-build-branch", truncate = true, style = { color = "#402b08" } } },
			if build.pr
				then ui.row {
					class = "sheprd-link",
					shrink = false,
					msg = { open_build_pr = true },
					to = LINK,
					tooltip = "Open the pull request",
					ui.text { `PR #{build.pr}`, style = { color = "#402b08" } },
				}
				else false,
		}
	end,
}
