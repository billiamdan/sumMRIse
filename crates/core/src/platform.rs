use crate::error::Result;
use std::future::Future;

pub trait PlatformGui {
    fn new(app: crate::app::SumMRIseApp) -> impl Future<Output = Result<Self>>
    where
        Self: Sized;
    
    fn run(self) -> impl Future<Output = Result<()>>;
    
    fn show_folder_picker(&self) -> Result<Option<String>>;
    
    fn show_about_dialog(&self) -> Result<()>;
    
    fn show_settings_dialog(&self) -> Result<()>;
}

pub fn current_platform() -> &'static str {
    #[cfg(target_os = "linux")]
    return "linux";
    
    #[cfg(target_os = "macos")]
    return "macos";
    
    #[cfg(target_os = "windows")]
    return "windows";
    
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    return "unknown";
}
