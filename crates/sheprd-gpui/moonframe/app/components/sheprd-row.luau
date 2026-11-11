-- One row of the left pane's lists, its children stacked as its lines and
-- sized by them, highlighted as the configured layout
-- marks the selected row: flat layouts fill it edge to edge, rounded ones
-- (and Orca's cards) inset it, round it and outline it, and Superset adds a
-- stripe down its leading edge. Unselected rows keep a clear border, so
-- selecting one never moves its text.

type Props = {
	class: string?,
	selected: boolean?,
	layout: any,
	theme: any,
	indent: number?,
	card: boolean?,
	stripe: boolean?,
	msg: any,
	context: any,
	carry: any,
	accept: any,
	drop: string?,
	to: string?,
	tooltip: string?,
}

-- `color` with no alpha, so a clear border keeps its place.
local function clear(color: string?): string?
	return if color then `{color}00` else nil
end

return component {
	title = "Sheprd Row",
	category = "Sheprd",
	icon = "rows-2",
	description = "A row of the left pane with its children stacked as its lines, highlighted as the configured sidebar layout marks the selected row.",
	when = "For each worktree or agent row, so every list follows the same layout.",
	props = {
		["class"] = "the row's own class, for hover rules and Inspect",
		["selected"] = "whether the row is the focused one",
		["layout"] = "`app.sheprd.layout`: spacing, inset, radius and highlight",
		["theme"] = "`app.sheprd_theme`, for the colors",
		["indent"] = "how far it steps in, for a worktree under its checkout",
		["card"] = "draw it as a card (Orca) whatever the layout's highlight",
		["stripe"] = "mark the selected row with a stripe down its leading edge (Superset)",
		["msg"] = "what a click sends",
		["context"] = "what a right click sends, to open a menu",
		["carry"] = "what the row carries when dragged onto a row with `accept`",
		["accept"] = "what a row dragged over this one sends, with `phase`, `carry` and `y`",
		["drop"] = "\"before\" or \"after\": where a dragged row would land, drawn as a line",
		["to"] = "where the click goes",
		["tooltip"] = "text on hover",
	},
	example = { layout = { padding = 8, line = 16 }, theme = {}, selected = true },

	build = function(props: Props)
		local layout = props.layout or {}
		local theme = props.theme or {}
		local selected = props.selected == true
		local outline = props.card or layout.outline
		local inset = if props.card then 4 else layout.inset or 0
		local style: any = {
			padding = { x = layout.padding or 8, y = (layout.row_padding or 0) + (layout.style_padding or 0) },
			margin = { left = inset + (props.indent or 0), right = inset, y = (layout.spacing or 0) / 2 },
		}
		if outline then
			style.radius = if props.card then 6 else layout.radius
			style.border = "all"
			style.border_color = if selected then theme.muted else clear(theme.sidebar)
			style.background = if selected and theme.foreground then `{theme.foreground}12` else nil
		elseif props.stripe then
			style.border = "left"
			style.border_color = if selected then theme.accent else clear(theme.sidebar)
			style.background = if selected then theme.active else nil
		else
			style.background = if selected then theme.active else nil
		end
		local row: any = {
			class = `sheprd-row {props.class or ""}`,
			selected = selected,
			gap = "none",
			shrink = false,
			msg = props.msg,
			context = props.context,
			carry = props.carry,
			accept = props.accept,
			to = props.to,
			tooltip = props.tooltip,
			style = style,
		}
		-- Where a dragged row would land, as a line across this one's edge.
		local line = ui.row { class = "sheprd-row-drop", height = 2, shrink = false, style = { background = theme.accent } }
		if props.drop == "before" then
			table.insert(row, line)
		end
		for _, child in ipairs(props :: any) do
			table.insert(row, child)
		end
		if props.drop == "after" then
			table.insert(row, line)
		end
		return ui.column(row)
	end,
}
