pub mod error;
pub mod parser;
pub mod utils;

pub use error::{DicomError, Result};
pub use parser::DicomParser;

use summrise_core::models::MedicalStudy;

pub async fn load_study_from_folder(folder_path: &str) -> Result<MedicalStudy> {
    let parser = DicomParser::new();
    parser.load_study_from_folder(folder_path).await
}
