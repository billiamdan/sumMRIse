use std::path::PathBuf;

use summrise_i18n::tr;

use crate::media::ArchiveProblem;

/// Errors produced while importing DICOM media. They are shown to the user
/// as they are, in the language of the interface.
#[derive(Debug, thiserror::Error)]
pub enum DicomError {
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        reason: String,
    },
    NothingFound(PathBuf),
    Cancelled,
    DecodeFailed(String),
    /// One image could not be decoded; the text is the decoder's reason.
    Image(String),
    /// An archive or a disc image that cannot be opened as it is. The text
    /// tells the user why, and what to do instead.
    Archive {
        path: PathBuf,
        problem: ArchiveProblem,
    },
}

impl std::fmt::Display for DicomError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::Io { path, source } => tr!("cannot read {path}: {reason}", path = path.display(), reason = source),
            Self::Parse { path, reason } => {
                tr!("{path}: not a readable DICOM file ({reason})", path = path.display(), reason = reason)
            }
            Self::NothingFound(path) => tr!("no DICOM images were found in {path}", path = path.display()),
            Self::Cancelled => tr!("the operation was cancelled").to_owned(),
            Self::DecodeFailed(reason) => {
                tr!("none of the images in this series could be decoded: {reason}", reason = reason)
            }
            Self::Image(reason) => reason.clone(),
            Self::Archive { path, problem } => problem.message(path),
        };
        f.write_str(&text)
    }
}

pub type Result<T, E = DicomError> = std::result::Result<T, E>;
