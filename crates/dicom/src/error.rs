use thiserror::Error;

#[derive(Error, Debug)]
pub enum DicomError {
    #[error("DICOM parsing error: {0}")]
    ParsingError(#[from] dicom::object::ReadError),
    
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    
    #[error("Invalid DICOM file: {0}")]
    InvalidFile(String),
    
    #[error("Missing required tag: {0}")]
    MissingTag(String),
    
    #[error("Unsupported transfer syntax: {0}")]
    UnsupportedTransferSyntax(String),
    
    #[error("Image decoding error: {0}")]
    ImageDecodingError(String),
}

pub type Result<T> = std::result::Result<T, DicomError>;
