-- A section's name over its list in the left pane, with an optional control
-- at its end, spelt and spaced as the configured layout's headings are.

type Props = {
	text: string,
	side: any,
	theme: any,
	layout: any,
}

return component {
	title = "Sheprd Subhead",
	category = "Sheprd",
	icon = "heading",
	description = "A quiet section name over a list, with room for one control at its end.",
	when = "Over each list of the left pane: spaces, agents.",
	props = {
		["text"] = "the section's name, in lower case",
		["side"] = "an element at its end, such as an icon button or the sort's name",
		["theme"] = "`app.sheprd_theme`, for the colors",
		["layout"] = "`app.sheprd.layout`: rounded layouts title-case the name",
	},
	example = { text = "agents", theme = {} },

	build = function(props: Props)
		local theme = props.theme or {}
		local layout = props.layout or {}
		local text = props.text
		if layout.title_case then
			text = string.upper(string.sub(text, 1, 1)) .. string.sub(text, 2)
		end
		return ui.row {
			class = "sheprd-subhead",
			align = "center",
			shrink = false,
			style = { padding = { x = (layout.padding or 8) + (layout.inset or 0), y = 4 } },
			ui.text { text, style = { color = theme.muted } },
			ui.spacer(),
			props.side or false,
		}
	end,
}
