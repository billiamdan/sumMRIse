use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, Box, Orientation};
use log::info;
use summrise_core::{
    app::SumMRIseApp,
    error::Result,
    platform::PlatformGui,
};

pub struct LinuxGui {
    #[allow(dead_code)]
    app: SumMRIseApp,
    gtk_app: Application,
    window: ApplicationWindow,
}

impl LinuxGui {
    pub fn new(app: SumMRIseApp) -> Result<Self> {
        info!("Initializing Linux GUI");

        // Create GTK Application first
        let gtk_app = Application::builder()
            .application_id("com.summrise.app")
            .build();

        // Create window
        let window = ApplicationWindow::builder()
            .title("SumMRIse - Medical Imaging Viewer")
            .default_width(1200)
            .default_height(800)
            .application(&gtk_app)
            .build();

        // Create a simple layout
        let main_box = Box::new(Orientation::Vertical, 0);
        window.set_child(Some(&main_box));

        Ok(Self { 
            app, 
            gtk_app,
            window 
        })
    }

    pub fn new_minimal() -> Result<Self> {
        info!("Initializing minimal Linux GUI");

        // Create GTK Application first
        let gtk_app = Application::builder()
            .application_id("com.summrise.app")
            .build();

        // Create window without setting application initially
        let window = ApplicationWindow::builder()
            .title("SumMRIse - Medical Imaging Viewer")
            .default_width(1200)
            .default_height(800)
            .build();

        // Set the application after creation
        window.set_application(Some(&gtk_app));

        // Create a simple layout
        let main_box = Box::new(Orientation::Vertical, 0);
        window.set_child(Some(&main_box));

        // Create a dummy app for now
        let app = SumMRIseApp {
            config: Default::default(),
            studies: std::collections::HashMap::new(),
            llm_config: None,
        };

        Ok(Self { 
            app, 
            gtk_app,
            window 
        })
    }

    pub fn run(self) -> Result<()> {
        info!("Starting Linux GUI application");
        
        // Connect the activate signal
        self.gtk_app.connect_activate(move |_app| {
            info!("GTK Application activated");
        });

        // Present the window
        self.window.present();

        // Run the application
        self.gtk_app.run();
        Ok(())
    }

    pub fn show_folder_picker(&self) -> Result<Option<String>> {
        // Placeholder implementation
        Ok(Some("/tmp/test".to_string()))
    }

    pub fn show_about_dialog(&self) -> Result<()> {
        info!("About dialog requested");
        Ok(())
    }

    pub fn show_settings_dialog(&self) -> Result<()> {
        info!("Settings dialog requested");
        Ok(())
    }
}

impl PlatformGui for LinuxGui {
    async fn new(app: SumMRIseApp) -> Result<Self> {
        Ok(Self::new(app)?)
    }

    async fn run(self) -> Result<()> {
        self.run()
    }

    fn show_folder_picker(&self) -> Result<Option<String>> {
        self.show_folder_picker()
    }

    fn show_about_dialog(&self) -> Result<()> {
        self.show_about_dialog()
    }

    fn show_settings_dialog(&self) -> Result<()> {
        self.show_settings_dialog()
    }
}