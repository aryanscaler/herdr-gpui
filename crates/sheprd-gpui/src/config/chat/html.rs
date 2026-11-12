//! `[chat] render_html`: how an agent's HTML fence starts out. Every fence
//! keeps a button that switches between its page and its code, whatever
//! this says.

use serde::Deserialize;

/// Declares each mode once: its variant, the config value that selects it,
/// and how Settings titles it, in the order Settings lists them.
macro_rules! html_renders {
    ($($(#[$doc:meta])* $variant:ident => $name:literal, $label:literal;)+) => {
        /// How an HTML fence starts out.
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
        pub enum HtmlRender {
            $($(#[$doc])* $variant,)+
        }

        impl HtmlRender {
            /// Every mode, in the order Settings lists them.
            pub const ALL: &'static [Self] = &[$(Self::$variant,)+];

            /// `ALL`'s names, for errors that list what a config may say.
            const NAMES: &'static [&'static str] = &[$($name,)+];

            /// The config value that selects it.
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $name,)+
                }
            }

            /// How Settings titles it.
            pub fn label(self) -> String {
                crate::i18n::message(self.message(), &[])
            }

            /// The message naming it.
            pub const fn message(self) -> &'static str {
                match self {
                    $(Self::$variant => $label,)+
                }
            }
        }
    };
}

html_renders! {
    /// The page loads in the message as soon as it is drawn, without a click.
    Always => "always", "chat-html-render-always";
    /// The fence shows its code.
    Never => "never", "chat-html-render-never";
    /// A card that loads the page when its button is clicked.
    #[default]
    Default => "default", "chat-html-render-default";
}

impl<'de> Deserialize<'de> for HtmlRender {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::ALL
            .iter()
            .copied()
            .find(|mode| mode.name() == name)
            .ok_or_else(|| serde::de::Error::unknown_variant(&name, Self::NAMES))
    }
}
