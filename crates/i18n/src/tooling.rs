//! Keeping the catalogues in step with the source code, for the
//! completeness test (`crates/i18n/tests/completeness.rs`) and
//! `cargo xtask i18n`: the messages of the source code ([`scan_workspace`]),
//! the template and the up-to-date PO file of a language ([`template`],
//! [`update`]), and what is missing, untranslated, obsolete or wrong in a PO
//! file ([`check`]). Not used by the application itself.

mod scan;
mod update;

pub use scan::{Message, ScanError, scan_source, scan_workspace, source_files};
pub use update::{Check, check, header, placeholders, template, update};
