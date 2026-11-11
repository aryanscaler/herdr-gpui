-- Rows answer the pointer as Sheprd's do. How a selected row is marked
-- depends on the sidebar layout, so `sheprd-row` draws it from
-- `app.sheprd.layout`; colors here are the toolkit's, which follow Sheprd's
-- theme: `muted` is its selection, `muted-foreground` its quiet text.
return style {
	rules = {
		[".worktree-tile"] = { radius = 6 },
		[".worktree-tile:selected"] = { background = "muted" },
		[".worktree-row:hover"] = { background = "secondary" },
		[".agent-row:hover"] = { background = "secondary" },
		[".activity-tile:hover"] = { background = "secondary" },
		[".status-item:hover"] = { opacity = 0.75 },
		[".sheprd-link:hover"] = { opacity = 0.75 },
		-- The left pane's edge: a hairline, lit as a split's handle is while
		-- the pointer is over it, to say it can be dragged.
		[".sheprd-left-edge-line"] = { background = "border" },
		[".sheprd-left-edge:hover"] = { background = "accent/40" },
	},
}
