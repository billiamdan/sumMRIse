use std::path::PathBuf;

/// Errors produced while importing DICOM media.
#[derive(Debug, thiserror::Error)]
pub enum DicomError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path}: not a readable DICOM file ({reason})")]
    Parse { path: PathBuf, reason: String },
    #[error("no DICOM images were found in {0}")]
    NothingFound(PathBuf),
    #[error("the operation was cancelled")]
    Cancelled,
    #[error("none of the images in this series could be decoded: {0}")]
    DecodeFailed(String),
}

pub type Result<T, E = DicomError> = std::result::Result<T, E>;
