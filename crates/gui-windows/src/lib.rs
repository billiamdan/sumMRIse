// Windows GUI implementation placeholder
// This will be implemented using WinUI/Win32

use anyhow::Result;
use summrise_core::{app::SumMRIseApp, platform::PlatformGui};

pub struct WindowsGui {
    // Windows-specific implementation
}

impl WindowsGui {
    pub async fn new(_app: SumMRIseApp) -> Result<Self> {
        todo!("Windows GUI not yet implemented")
    }

    pub async fn run(self) -> Result<()> {
        todo!("Windows GUI not yet implemented")
    }
}

impl PlatformGui for WindowsGui {
    async fn new(app: SumMRIseApp) -> Result<Self> {
        Self::new(app).await
    }

    async fn run(self) -> Result<()> {
        self.run().await
    }

    fn show_folder_picker(&self) -> Result<Option<String>> {
        todo!("Windows GUI not yet implemented")
    }

    fn show_about_dialog(&self) -> Result<()> {
        todo!("Windows GUI not yet implemented")
    }

    fn show_settings_dialog(&self) -> Result<()> {
        todo!("Windows GUI not yet implemented")
    }
}

