# Settings > General, beyond the rows search finds (settings.ftl).

## Groups on the page.

settings-general-section-interface = Interface
settings-general-section-features = Features
settings-general-section-clipboard = Clipboard
settings-general-section-worktrees = Worktrees
settings-general-section-browser-skill = Browser skill
settings-general-section-startup = Startup
settings-general-section-updates = Updates

## Rows.

# The language picker's first choice. $language is the name, in itself, of
# the language the system's preferences pick (English, עברית).
settings-general-language-system = System ({ $language })
# A tag the `language` setting names that has no messages. $tag is the BCP 47
# tag (fr-CA); $language names, in itself, the language shown instead.
settings-general-language-unavailable = { $tag }, shown in { $language }

settings-general-menu-bar = Show menu bar
settings-general-menu-bar-description = The row of menus at the top of the window. While hidden, F10 opens the menus over the window and Toggle Menu Bar brings the row back.
settings-general-window-controls = Show window controls
settings-general-window-controls-description = Draw minimize, maximize, and close buttons in the menu bar, or the title bar while the menu bar is hidden. Disable server-side decorations in your window manager to avoid a double bar.
settings-general-browser-skill-missing = Not installed.
settings-general-browser-skill-install = Install
settings-general-browser-skill-remove = Remove
# Windows' own Startup switch has turned the entry off.
settings-general-launch-at-login-disabled = Turned off in Settings > Apps > Startup. Turn it back on there.
settings-general-error-launch-at-login = Launch at login: { $error }
