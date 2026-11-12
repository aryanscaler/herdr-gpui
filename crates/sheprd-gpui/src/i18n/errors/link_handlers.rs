use super::Localize;
use crate::{i18n::fl, link_handlers::Error};

impl Localize for Error {
    fn localize(&self) -> String {
        match self {
            Self::HostChanged => fl!("error-link-handlers-host-changed"),
            Self::Run(error) => error.localize(),
            Self::Io(error) => fl!("error-link-handlers-transport", reason = error.to_string()),
            Self::Invalid(reason) => fl!("error-link-handlers-config", reason = *reason),
            Self::Pattern(error) => fl!("error-link-handlers-config", reason = error.to_string()),
            Self::Manifest(error) => fl!("error-link-handlers-config", reason = error.to_string()),
            Self::Utf8(error) => fl!("error-link-handlers-config", reason = error.to_string()),
        }
    }
}
