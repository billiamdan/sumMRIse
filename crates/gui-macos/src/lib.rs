// macOS GUI implementation placeholder
// This will be implemented using Cocoa/Swift bindings

use anyhow::Result;
use summrise_core::{app::SumMRIseApp, platform::PlatformGui};

pub struct MacOSGui {
    // macOS-specific implementation
}

impl MacOSGui {
    pub async fn new(_app: SumMRIseApp) -> Result<Self> {
        todo!("macOS GUI not yet implemented")
    }

    pub async fn run(self) -> Result<()> {
        todo!("macOS GUI not yet implemented")
    }
}

impl PlatformGui for MacOSGui {
    async fn new(app: SumMRIseApp) -> Result<Self> {
        Self::new(app).await
    }

    async fn run(self) -> Result<()> {
        self.run().await
    }

    fn show_folder_picker(&self) -> Result<Option<String>> {
        todo!("macOS GUI not yet implemented")
    }

    fn show_about_dialog(&self) -> Result<()> {
        todo!("macOS GUI not yet implemented")
    }

    fn show_settings_dialog(&self) -> Result<()> {
        todo!("macOS GUI not yet implemented")
    }
}

