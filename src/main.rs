use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Box, Orientation, 
    MenuButton, PopoverMenu, HeaderBar,
    Button, Label,
    FileChooserDialog, FileChooserAction, ResponseType,
    Popover, Entry, Switch, SpinButton,
    Frame, DrawingArea, ComboBoxText, Scale
};
use log::info;
use summrise_dicom::DicomParser;
use std::cell::RefCell;
use std::rc::Rc;
use serde::{Serialize, Deserialize};
use std::fs;

// Cache structure for DICOM data
#[derive(Serialize, Deserialize, Clone)]
struct DicomCache {
    folder_path: String,
    study: summrise_core::models::MedicalStudy,
    timestamp: u64,
}

// Cache management
fn get_cache_dir() -> std::path::PathBuf {
    let mut cache_dir = dirs::cache_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    cache_dir.push("summrise");
    cache_dir
}

fn save_to_cache(folder_path: &str, study: &summrise_core::models::MedicalStudy) -> Result<(), std::boxed::Box<dyn std::error::Error + Send + Sync>> {
    let cache_dir = get_cache_dir();
    fs::create_dir_all(&cache_dir)?;
    
    let cache_file = cache_dir.join("dicom_cache.json");
    let cache = DicomCache {
        folder_path: folder_path.to_string(),
        study: study.clone(),
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    };
    
    let json = serde_json::to_string_pretty(&cache)?;
    fs::write(cache_file, json)?;
    info!("DICOM data cached successfully");
    Ok(())
}

fn load_from_cache(folder_path: &str) -> Option<summrise_core::models::MedicalStudy> {
    let cache_dir = get_cache_dir();
    let cache_file = cache_dir.join("dicom_cache.json");
    
    if !cache_file.exists() {
        return None;
    }
    
    match fs::read_to_string(&cache_file) {
        Ok(json) => {
            match serde_json::from_str::<DicomCache>(&json) {
                Ok(cache) => {
                    // Check if the cached data is for the same folder
                    if cache.folder_path == folder_path {
                        // Check if cache is not too old (24 hours)
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        
                        if now - cache.timestamp < 86400 { // 24 hours
                            info!("Loading DICOM data from cache");
                            return Some(cache.study);
                        } else {
                            info!("Cache is too old, will reload");
                        }
                    } else {
                        info!("Cache is for different folder, will reload");
                    }
                }
                Err(e) => {
                    info!("Failed to parse cache: {}", e);
                }
            }
        }
        Err(e) => {
            info!("Failed to read cache: {}", e);
        }
    }
    
    None
}

fn main() {
    env_logger::init();
    info!("Starting SumMRIse - Medical Imaging Viewer");

    let app = Application::builder()
        .application_id("com.summrise.app")
        .build();

    app.connect_activate(|app| {
        info!("GTK4 app activated");
        create_main_window(app);
    });

    info!("Running GTK4 app...");
    app.run();
    info!("GTK4 app finished.");
}

fn create_main_window(app: &Application) {
    // Create main window
    let window = ApplicationWindow::builder()
        .title("SumMRIse - Medical Imaging Viewer")
        .default_width(1400)
        .default_height(900)
        .application(app)
        .build();

    // Create header bar with menu
    let header_bar = HeaderBar::new();
    window.set_titlebar(Some(&header_bar));

    // Create menu button
    let menu_button = MenuButton::builder()
        .icon_name("open-menu-symbolic")
        .build();

    // Create menu
    let menu_model = gio::Menu::new();
    let menu = PopoverMenu::from_model(Some(&menu_model));
    
    // File menu
    let file_menu = gio::Menu::new();
    file_menu.append(Some("Open DICOM Folder"), Some("app.open_folder"));
    file_menu.append(Some("Export Image"), Some("app.export_image"));
    file_menu.append(Some("Quit"), Some("app.quit"));
    menu_model.append_submenu(Some("File"), &file_menu);

    // View menu
    let view_menu = gio::Menu::new();
    view_menu.append(Some("3D View"), Some("app.view_3d"));
    view_menu.append(Some("Slice View"), Some("app.view_slice"));
    view_menu.append(Some("Reset View"), Some("app.reset_view"));
    menu_model.append_submenu(Some("View"), &view_menu);

    // Analysis menu
    let analysis_menu = gio::Menu::new();
    analysis_menu.append(Some("Configure AI"), Some("app.configure_ai"));
    analysis_menu.append(Some("Run Analysis"), Some("app.run_analysis"));
    menu_model.append_submenu(Some("Analysis"), &analysis_menu);

    // Help menu
    let help_menu = gio::Menu::new();
    help_menu.append(Some("About"), Some("app.about"));
    menu_model.append_submenu(Some("Help"), &help_menu);

    menu.set_menu_model(Some(&menu_model));
    menu_button.set_popover(Some(&menu));
    header_bar.pack_end(&menu_button);

    // Create file picker button
    let open_folder_button = Button::builder()
        .icon_name("folder-open-symbolic")
        .tooltip_text("Open DICOM Folder")
        .build();
    header_bar.pack_start(&open_folder_button);

    // Create settings button
    let settings_button = Button::builder()
        .icon_name("preferences-system-symbolic")
        .tooltip_text("Settings")
        .build();
    header_bar.pack_start(&settings_button);

    // Create main content area
    let main_box = Box::new(Orientation::Vertical, 0);
    window.set_child(Some(&main_box));

    // Shared state for current study data
    let current_study = Rc::new(RefCell::new(None::<summrise_core::models::MedicalStudy>));
    
    // Store references to drawing areas for redraws
    let drawing_areas = Rc::new(RefCell::new(Vec::<DrawingArea>::new()));

    // Create welcome screen (shown when no data is loaded)
    let welcome_screen = create_welcome_screen(&current_study, &drawing_areas);
    main_box.append(&welcome_screen);

    // Connect button signals
    let window_clone = window.clone();
    let main_box_clone1 = main_box.clone();

    let current_study_clone = current_study.clone();
    let drawing_areas_clone = drawing_areas.clone();
    open_folder_button.connect_clicked(move |_| {
        info!("Open folder button clicked");
        show_folder_picker(&window_clone, &current_study_clone, &drawing_areas_clone, &main_box_clone1);
    });

    // Settings popup
    let settings_popover = Popover::new();
    let settings_content = create_settings_content();
    settings_popover.set_child(Some(&settings_content));
    settings_popover.set_parent(&settings_button);
    
    settings_button.connect_clicked(move |_| {
        info!("Settings button clicked");
        settings_popover.popup();
    });

    // Connect menu actions
    let action_group = gio::SimpleActionGroup::new();
    
    let open_folder_action = gio::SimpleAction::new("open_folder", None);
    let window_clone2 = window.clone();
    let current_study_clone2 = current_study.clone();
    let drawing_areas_clone2 = drawing_areas.clone();
    let main_box_clone2 = main_box.clone();
    open_folder_action.connect_activate(move |_, _| {
        info!("Open folder menu item clicked");
        show_folder_picker(&window_clone2, &current_study_clone2, &drawing_areas_clone2, &main_box_clone2);
    });
    action_group.add_action(&open_folder_action);

    let quit_action = gio::SimpleAction::new("quit", None);
    let app_clone = app.clone();
    quit_action.connect_activate(move |_, _| {
        info!("Quit menu item clicked");
        app_clone.quit();
    });
    action_group.add_action(&quit_action);

    let about_action = gio::SimpleAction::new("about", None);
    about_action.connect_activate(move |_, _| {
        info!("About menu item clicked");
        // TODO: Implement about dialog
    });
    action_group.add_action(&about_action);

    window.insert_action_group("app", Some(&action_group));

    window.present();
}

fn create_welcome_screen(
    _current_study: &Rc<RefCell<Option<summrise_core::models::MedicalStudy>>>,
    _drawing_areas: &Rc<RefCell<Vec<DrawingArea>>>
) -> Box {
    let content = Box::new(Orientation::Vertical, 20);
    content.set_margin_top(40);
    content.set_margin_bottom(40);
    content.set_margin_start(40);
    content.set_margin_end(40);

    // App title and description
    let title = Label::new(Some("SumMRIse"));
    title.add_css_class("title-1");
    title.set_margin_bottom(20);
    content.append(&title);

    let subtitle = Label::new(Some("Advanced Medical Imaging Viewer & AI Analysis Platform"));
    subtitle.add_css_class("title-2");
    subtitle.set_margin_bottom(30);
    content.append(&subtitle);

    // App purpose explanation
    let purpose_frame = Frame::new(Some("About SumMRIse"));
    purpose_frame.add_css_class("card");
    let purpose_content = Box::new(Orientation::Vertical, 15);
    purpose_content.set_margin_top(20);
    purpose_content.set_margin_bottom(20);
    purpose_content.set_margin_start(20);
    purpose_content.set_margin_end(20);

    let purpose_text = Label::new(Some(
        "SumMRIse is a comprehensive medical imaging platform designed for healthcare professionals, \
        researchers, and medical students. Our application provides advanced DICOM viewing capabilities \
        with AI-powered analysis features to enhance diagnostic workflows and medical education.\n\n\
        Key Features:\n\
        • Multi-planar DICOM image viewing (Axial, Coronal, Sagittal)\n\
        • Advanced window/level controls for optimal image contrast\n\
        • 3D volume rendering capabilities\n\
        • AI-powered medical image analysis\n\
        • Intuitive user interface with professional-grade tools\n\
        • Fast loading with intelligent caching system"
    ));
    purpose_text.add_css_class("body");
    purpose_text.set_wrap(true);
    purpose_content.append(&purpose_text);

    purpose_frame.set_child(Some(&purpose_content));
    content.append(&purpose_frame);

    // Quick start section
    let quickstart_frame = Frame::new(Some("Quick Start"));
    quickstart_frame.add_css_class("card");
    let quickstart_content = Box::new(Orientation::Vertical, 15);
    quickstart_content.set_margin_top(20);
    quickstart_content.set_margin_bottom(20);
    quickstart_content.set_margin_start(20);
    quickstart_content.set_margin_end(20);

    let quickstart_text = Label::new(Some(
        "To begin viewing your medical images:\n\n\
        1. Click the 'Open DICOM Folder' button in the header bar\n\
        2. Select a folder containing DICOM files (.dcm)\n\
        3. Wait for the images to load (first time may take a moment)\n\
        4. View your images directly in the main area\n\
        5. Use the settings panel to adjust image display parameters\n\n\
        Your data will be cached for faster loading on subsequent visits!"
    ));
    quickstart_text.add_css_class("body");
    quickstart_text.set_wrap(true);
    quickstart_content.append(&quickstart_text);

    // Add a prominent "Open DICOM Folder" button
    let open_button = Button::builder()
        .label("Open DICOM Folder")
        .css_classes(vec!["suggested-action".to_string(), "pill".to_string()])
        .build();
    open_button.set_margin_top(20);
    quickstart_content.append(&open_button);

    quickstart_frame.set_child(Some(&quickstart_content));
    content.append(&quickstart_frame);

    // Recent studies section (will be populated when data is loaded)
    let recent_frame = Frame::new(Some("Recent Studies"));
    recent_frame.add_css_class("card");
    let recent_content = Box::new(Orientation::Vertical, 10);
    recent_content.set_margin_top(20);
    recent_content.set_margin_bottom(20);
    recent_content.set_margin_start(20);
    recent_content.set_margin_end(20);

    let recent_text = Label::new(Some("No recent studies found. Load a DICOM folder to see your studies here."));
    recent_text.add_css_class("body");
    recent_text.set_wrap(true);
    recent_content.append(&recent_text);

    recent_frame.set_child(Some(&recent_content));
    content.append(&recent_frame);

    // Connect the open button
    open_button.connect_clicked(move |_| {
        info!("Open DICOM folder button clicked from welcome screen");
        // This will be connected to the main window's folder picker
        // For now, we'll show a message
        info!("Please use the folder button in the header bar to open DICOM files");
    });

    content
}

fn show_folder_picker(
    parent: &ApplicationWindow, 
    current_study: &Rc<RefCell<Option<summrise_core::models::MedicalStudy>>>,
    drawing_areas: &Rc<RefCell<Vec<DrawingArea>>>,
    main_box: &Box
) {
    let dialog = FileChooserDialog::builder()
        .title("Select DICOM Folder")
        .modal(true)
        .action(FileChooserAction::SelectFolder)
        .build();

    dialog.add_button("Cancel", ResponseType::Cancel);
    dialog.add_button("Select", ResponseType::Accept);

    dialog.set_transient_for(Some(parent));

    let parent_clone = parent.clone();
    let current_study_clone = current_study.clone();
    let drawing_areas_clone = drawing_areas.clone();
    let main_box_clone = main_box.clone();
    dialog.connect_response(move |dialog, response| {
        if response == ResponseType::Accept {
            if let Some(folder) = dialog.file() {
                if let Some(path) = folder.path() {
                    info!("Selected folder: {:?}", path);
                    show_folder_info(&parent_clone, &path.to_string_lossy(), &current_study_clone, &drawing_areas_clone, &main_box_clone);
                }
            }
        }
        dialog.close();
    });

    dialog.present();
}

fn show_folder_info(
    parent: &ApplicationWindow, 
    folder_path: &str, 
    current_study: &Rc<RefCell<Option<summrise_core::models::MedicalStudy>>>,
    drawing_areas: &Rc<RefCell<Vec<DrawingArea>>>,
    main_box: &Box
) {
    // Create a progress dialog
    let progress_dialog = gtk4::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .title("Loading DICOM Files")
        .message_type(gtk4::MessageType::Info)
        .text("Loading DICOM files from folder...")
        .secondary_text("Please wait while we parse the medical images.")
        .build();

    progress_dialog.present();

    // Load DICOM files asynchronously with caching
    let parent_clone = parent.clone();
    let folder_path = folder_path.to_string();
    let current_study_clone = current_study.clone();
    let drawing_areas_clone = drawing_areas.clone();
    let main_box_clone = main_box.clone();
    
    glib::MainContext::default().spawn_local(async move {
        // First, try to load from cache
        if let Some(cached_study) = load_from_cache(&folder_path) {
            progress_dialog.close();
            info!("Loaded DICOM data from cache");
                show_study_info(&parent_clone, &cached_study);
                update_study_state(&cached_study, &current_study_clone, &drawing_areas_clone, &main_box_clone);
            return;
        }
        
        // If not in cache, load from files
        info!("Loading DICOM files from disk (not in cache)");
        let parser = DicomParser::new();
        
        match parser.load_study_from_folder(&folder_path).await {
            Ok(study) => {
                progress_dialog.close();
                
                // Save to cache for next time
                if let Err(e) = save_to_cache(&folder_path, &study) {
                    info!("Failed to save to cache: {}", e);
                }
                
                show_study_info(&parent_clone, &study);
                update_study_state(&study, &current_study_clone, &drawing_areas_clone, &main_box_clone);
            }
            Err(e) => {
                progress_dialog.close();
                show_error_dialog(&parent_clone, &format!("Failed to load DICOM files: {}", e));
            }
        }
    });
}

fn show_study_info(parent: &ApplicationWindow, study: &summrise_core::models::MedicalStudy) {
    let info_text = format!(
        "Study loaded successfully!\n\n\
        Patient ID: {}\n\
        Study Date: {}\n\
        Study Description: {}\n\
        Modality: {}\n\
        Number of Series: {}",
        study.patient_id.as_deref().unwrap_or("Unknown"),
        study.study_date.as_deref().unwrap_or("Unknown"),
        study.study_description.as_deref().unwrap_or("Unknown"),
        study.modality,
        study.series.len()
    );

    // Create detailed study information for the welcome tab
    let detailed_info = format!(
        "Patient ID: {}\n\
        Study Date: {}\n\
        Study Description: {}\n\
        Modality: {}\n\
        Number of Series: {}\n\
        Study ID: {}",
        study.patient_id.as_deref().unwrap_or("Unknown"),
        study.study_date.as_deref().unwrap_or("Unknown"),
        study.study_description.as_deref().unwrap_or("Unknown"),
        study.modality,
        study.series.len(),
        study.id
    );

    // Add series details
    let mut series_info = String::new();
    for (i, series) in study.series.iter().enumerate() {
        series_info.push_str(&format!(
            "\n\nSeries {}:\n\
            - Description: {}\n\
            - Modality: {}\n\
            - Number of Instances: {}\n\
            - Series Number: {}",
            i + 1,
            series.series_description.as_deref().unwrap_or("Unknown"),
            series.modality,
            series.instances.len(),
            series.series_number.map(|n| n.to_string()).unwrap_or_else(|| "Unknown".to_string())
        ));
    }

    let _full_info = format!("{}{}", detailed_info, series_info);

    let dialog = gtk4::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .title("DICOM Study Loaded")
        .message_type(gtk4::MessageType::Info)
        .text(&info_text)
        .secondary_text("The study has been loaded and is ready for viewing and analysis.")
        .build();

    dialog.connect_response(|dialog, _| {
        dialog.close();
    });

    dialog.present();
}

fn show_error_dialog(parent: &ApplicationWindow, message: &str) {
    let dialog = gtk4::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .title("Error")
        .message_type(gtk4::MessageType::Error)
        .text(message)
        .build();

    dialog.connect_response(|dialog, _| {
        dialog.close();
    });

    dialog.present();
}
fn create_settings_content() -> Box {
    let settings_box = Box::new(Orientation::Vertical, 10);
    settings_box.set_margin_top(20);
    settings_box.set_margin_bottom(20);
    settings_box.set_margin_start(20);
    settings_box.set_margin_end(20);

    // Header with close button
    let header_box = Box::new(Orientation::Horizontal, 0);
    let settings_title = Label::new(Some("Settings"));
    settings_title.add_css_class("title-2");
    header_box.append(&settings_title);
    
    let close_button = Button::builder()
        .icon_name("window-close-symbolic")
        .tooltip_text("Close")
        .build();
    header_box.append(&close_button);
    
    settings_box.append(&header_box);

    // LLM Settings
    let llm_frame = Frame::new(None);
    llm_frame.add_css_class("card");
    let llm_box = Box::new(Orientation::Vertical, 10);
    llm_box.set_margin_top(10);
    llm_box.set_margin_bottom(10);
    llm_box.set_margin_start(10);
    llm_box.set_margin_end(10);

    let llm_title = Label::new(Some("AI Analysis Settings"));
    llm_title.add_css_class("title-3");
    llm_box.append(&llm_title);

    // API Key
    let api_key_box = Box::new(Orientation::Horizontal, 10);
    let api_key_label = Label::new(Some("API Key:"));
    api_key_box.append(&api_key_label);
    let api_key_entry = Entry::new();
    api_key_entry.set_placeholder_text(Some("Enter your OpenAI API key"));
    api_key_entry.set_text("sk-mocked-api-key-12345");
    api_key_box.append(&api_key_entry);
    llm_box.append(&api_key_box);

    // Model selection
    let model_box = Box::new(Orientation::Horizontal, 10);
    let model_label = Label::new(Some("Model:"));
    model_box.append(&model_label);
    let model_combo = ComboBoxText::new();
    model_combo.append_text("gpt-4");
    model_combo.append_text("gpt-3.5-turbo");
    model_combo.set_active(Some(0));
    model_box.append(&model_combo);
    llm_box.append(&model_box);

    // Max tokens
    let tokens_box = Box::new(Orientation::Horizontal, 10);
    let tokens_label = Label::new(Some("Max Tokens:"));
    tokens_box.append(&tokens_label);
    let tokens_spin = SpinButton::with_range(100.0, 4000.0, 100.0);
    tokens_spin.set_value(2000.0);
    tokens_box.append(&tokens_spin);
    llm_box.append(&tokens_box);

    // Temperature
    let temp_box = Box::new(Orientation::Horizontal, 10);
    let temp_label = Label::new(Some("Temperature:"));
    temp_box.append(&temp_label);
    let temp_scale = Scale::with_range(Orientation::Horizontal, 0.0, 2.0, 0.1);
    temp_scale.set_value(0.7);
    temp_box.append(&temp_scale);
    llm_box.append(&temp_box);

    llm_frame.set_child(Some(&llm_box));
    settings_box.append(&llm_frame);

    // Rendering Settings
    let render_frame = Frame::new(None);
    render_frame.add_css_class("card");
    let render_box = Box::new(Orientation::Vertical, 10);
    render_box.set_margin_top(10);
    render_box.set_margin_bottom(10);
    render_box.set_margin_start(10);
    render_box.set_margin_end(10);

    let render_title = Label::new(Some("Rendering Settings"));
    render_title.add_css_class("title-3");
    render_box.append(&render_title);

    // VSync
    let vsync_box = Box::new(Orientation::Horizontal, 10);
    let vsync_label = Label::new(Some("VSync:"));
    vsync_box.append(&vsync_label);
    let vsync_switch = Switch::new();
    vsync_switch.set_active(true);
    vsync_box.append(&vsync_switch);
    render_box.append(&vsync_box);

    // MSAA
    let msaa_box = Box::new(Orientation::Horizontal, 10);
    let msaa_label = Label::new(Some("MSAA Samples:"));
    msaa_box.append(&msaa_label);
    let msaa_combo = ComboBoxText::new();
    msaa_combo.append_text("1x");
    msaa_combo.append_text("2x");
    msaa_combo.append_text("4x");
    msaa_combo.append_text("8x");
    msaa_combo.set_active(Some(2));
    msaa_box.append(&msaa_combo);
    render_box.append(&msaa_box);

    render_frame.set_child(Some(&render_box));
    settings_box.append(&render_frame);

    // DICOM Settings
    let dicom_frame = Frame::new(None);
    dicom_frame.add_css_class("card");
    let dicom_box = Box::new(Orientation::Vertical, 10);
    dicom_box.set_margin_top(10);
    dicom_box.set_margin_bottom(10);
    dicom_box.set_margin_start(10);
    dicom_box.set_margin_end(10);

    let dicom_title = Label::new(Some("DICOM Settings"));
    dicom_title.add_css_class("title-3");
    dicom_box.append(&dicom_title);

    // Auto load
    let autoload_box = Box::new(Orientation::Horizontal, 10);
    let autoload_label = Label::new(Some("Auto Load:"));
    autoload_box.append(&autoload_label);
    let autoload_switch = Switch::new();
    autoload_switch.set_active(true);
    autoload_box.append(&autoload_switch);
    dicom_box.append(&autoload_box);

    // Cache size
    let cache_box = Box::new(Orientation::Horizontal, 10);
    let cache_label = Label::new(Some("Cache Size (MB):"));
    cache_box.append(&cache_label);
    let cache_spin = SpinButton::with_range(64.0, 2048.0, 64.0);
    cache_spin.set_value(512.0);
    cache_box.append(&cache_spin);
    dicom_box.append(&cache_box);

    dicom_frame.set_child(Some(&dicom_box));
    settings_box.append(&dicom_frame);

    // Action buttons
    let action_box = Box::new(Orientation::Horizontal, 10);
    let save_button = Button::builder()
        .label("Save")
        .css_classes(vec!["suggested-action".to_string()])
        .build();
    let reset_button = Button::builder()
        .label("Reset")
        .build();
    action_box.append(&save_button);
    action_box.append(&reset_button);
    settings_box.append(&action_box);

    // Connect close button
    close_button.connect_clicked(move |_| {
        info!("Settings popup closed");
        // The popover will close automatically when clicked outside
    });

    settings_box
}


fn render_dicom_image(cr: &cairo::Context, image_data: &summrise_core::models::ImageData, width: i32, height: i32) {
    let img_width = image_data.width as i32;
    let img_height = image_data.height as i32;
    
    info!("render_dicom_image called: image={}x{}, area={}x{}", img_width, img_height, width, height);
    
    if img_width == 0 || img_height == 0 || image_data.pixel_data.is_empty() {
        info!("Invalid image data: width={}, height={}, pixel_data_len={}", 
              img_width, img_height, image_data.pixel_data.len());
        return;
    }
    
    // Calculate scaling to fit image in drawing area
    let scale_x = width as f64 / img_width as f64;
    let scale_y = height as f64 / img_height as f64;
    let scale = scale_x.min(scale_y);
    
    let scaled_width = (img_width as f64 * scale) as i32;
    let scaled_height = (img_height as f64 * scale) as i32;
    
    // Center the image
    let offset_x = (width - scaled_width) / 2;
    let offset_y = (height - scaled_height) / 2;
    
    // Apply window/level transformation
    let window_center = image_data.window_center;
    let window_width = image_data.window_width;
    
    info!("Window/Level in render: Center={}, Width={}", window_center, window_width);
    
    // Check some sample pixel values
    if !image_data.pixel_data.is_empty() {
        let sample_pixels: Vec<u16> = image_data.pixel_data.iter().take(10).cloned().collect();
        info!("Sample pixel values: {:?}", sample_pixels);
    }
    
    // Create a Cairo image surface for efficient rendering
    let surface = cairo::ImageSurface::create(cairo::Format::Rgb24, img_width, img_height)
        .expect("Failed to create image surface");
    let surface_cr = cairo::Context::new(&surface).expect("Failed to create context");
    
    // Fill background with black
    surface_cr.set_source_rgb(0.0, 0.0, 0.0);
    surface_cr.paint().unwrap();
    
    // Convert pixel data to RGB format efficiently
    let mut rgb_data = Vec::with_capacity((img_width * img_height * 4) as usize);
    
    for pixel_value in &image_data.pixel_data {
        // Apply window/level transformation
        let normalized = (*pixel_value as f64 - window_center + window_width / 2.0) / window_width;
        let intensity = (normalized.max(0.0).min(1.0) * 255.0) as u8;
        
        // Add RGB values (Cairo uses BGRA format)
        rgb_data.push(intensity); // Blue
        rgb_data.push(intensity); // Green  
        rgb_data.push(intensity); // Red
        rgb_data.push(255);       // Alpha
    }
    
    // Create image surface from RGB data
    let stride = cairo::Format::Rgb24.stride_for_width(img_width as u32).unwrap();
    let image_surface = cairo::ImageSurface::create_for_data(
        rgb_data,
        cairo::Format::Rgb24,
        img_width,
        img_height,
        stride,
    ).expect("Failed to create image surface from data");
    
    // Draw the image surface to the context
    cr.save().unwrap();
    cr.translate(offset_x as f64, offset_y as f64);
    cr.scale(scale, scale);
    cr.set_source_surface(&image_surface, 0.0, 0.0).unwrap();
    cr.paint().unwrap();
    cr.restore().unwrap();
    
    // Add a visible border to help debug
    cr.set_source_rgb(1.0, 0.0, 0.0); // Red border
    cr.set_line_width(2.0);
    cr.rectangle(offset_x as f64, offset_y as f64, scaled_width as f64, scaled_height as f64);
    cr.stroke().unwrap();
    
    // Add text to show the image is there
    cr.set_source_rgb(0.0, 1.0, 0.0); // Green text
    cr.set_font_size(12.0);
    cr.move_to(offset_x as f64 + 5.0, offset_y as f64 + 20.0);
    cr.show_text(&format!("{}x{}", img_width, img_height)).unwrap();
    
    info!("Image surface rendered successfully: scale={}, offset=({},{}), scaled=({},{})", 
          scale, offset_x, offset_y, scaled_width, scaled_height);
}

fn update_study_state(
    study: &summrise_core::models::MedicalStudy, 
    current_study: &Rc<RefCell<Option<summrise_core::models::MedicalStudy>>>,
    drawing_areas: &Rc<RefCell<Vec<DrawingArea>>>,
    main_box: &Box
) {
    info!("Updating study state with {} series", study.series.len());
    if !study.series.is_empty() {
        let series = &study.series[0];
        info!("First series has {} instances", series.instances.len());
        if !series.instances.is_empty() {
            let instance = &series.instances[0];
            if let Some(image_data) = &instance.image_data {
                info!("First instance has image data: {}x{} pixels", image_data.width, image_data.height);
            } else {
                info!("First instance has no image data");
            }
        }
    }
    // Update the shared state
    *current_study.borrow_mut() = Some(study.clone());
    info!("Study state updated successfully");
    
    // Replace welcome screen with DICOM viewer
    create_dicom_viewer(study, current_study, drawing_areas, main_box);
    
    info!("DICOM viewer created and displayed");
}

fn create_dicom_viewer(
    study: &summrise_core::models::MedicalStudy,
    current_study: &Rc<RefCell<Option<summrise_core::models::MedicalStudy>>>,
    drawing_areas: &Rc<RefCell<Vec<DrawingArea>>>,
    main_box: &Box
) {
    // Clear the main box
    while let Some(child) = main_box.first_child() {
        main_box.remove(&child);
    }
    
    // Create DICOM viewer content
    let viewer_content = Box::new(Orientation::Vertical, 10);
    viewer_content.set_margin_top(20);
    viewer_content.set_margin_bottom(20);
    viewer_content.set_margin_start(20);
    viewer_content.set_margin_end(20);
    
    // Study info header
    let study_info_frame = Frame::new(Some("Study Information"));
    study_info_frame.add_css_class("card");
    let study_info_content = Box::new(Orientation::Vertical, 10);
    study_info_content.set_margin_top(15);
    study_info_content.set_margin_bottom(15);
    study_info_content.set_margin_start(15);
    study_info_content.set_margin_end(15);
    
    let study_info_text = format!(
        "Patient ID: {} | Study Date: {} | Modality: {} | Series: {}",
        study.patient_id.as_deref().unwrap_or("Unknown"),
        study.study_date.as_deref().unwrap_or("Unknown"),
        study.modality,
        study.series.len()
    );
    let study_info_label = Label::new(Some(&study_info_text));
    study_info_label.add_css_class("body");
    study_info_content.append(&study_info_label);
    
    study_info_frame.set_child(Some(&study_info_content));
    viewer_content.append(&study_info_frame);
    
    // Multiple projections viewer
    let study_title = study.study_description.as_deref().unwrap_or("DICOM Study");
    let projections_label = Label::new(Some(&format!("{} - Multiple Projections", study_title)));
    projections_label.add_css_class("title-2");
    projections_label.set_margin_bottom(20);
    viewer_content.append(&projections_label);
    
    let projections_container = Box::new(Orientation::Horizontal, 10);
    projections_container.set_margin_start(20);
    projections_container.set_margin_end(20);
    projections_container.set_margin_bottom(20);
    
    // Create drawing areas for each series (up to 3)
    let current_study_clone1 = current_study.clone();
    let current_study_clone2 = current_study.clone();
    let current_study_clone3 = current_study.clone();
    
    // First projection
    let projection1_frame = Frame::new(None);
    projection1_frame.add_css_class("card");
    let projection1_box = Box::new(Orientation::Vertical, 5);
    projection1_box.set_margin_top(10);
    projection1_box.set_margin_bottom(10);
    projection1_box.set_margin_start(10);
    projection1_box.set_margin_end(10);
    
    let projection1_label = Label::new(Some("Projection 1"));
    projection1_label.add_css_class("title-3");
    projection1_box.append(&projection1_label);
    
    let drawing1 = DrawingArea::new();
    drawing1.set_size_request(400, 500);
    drawing1.add_css_class("viewer");
    
    drawing1.set_draw_func(move |_, cr, width, height| {
        info!("Projection 1 drawing area redraw: {}x{}", width, height);
        cr.set_source_rgb(0.0, 0.0, 0.0);
        cr.paint().unwrap();
        if let Some(study) = current_study_clone1.borrow().as_ref() {
            if !study.series.is_empty() {
                let series = &study.series[0];
                if !series.instances.is_empty() {
                    let instance = &series.instances[0];
                    if let Some(image_data) = &instance.image_data {
                        info!("Projection 1: {}x{} pixels", image_data.width, image_data.height);
                        render_dicom_image(cr, image_data, width, height);
                        return;
                    }
                }
            }
        }
        cr.set_source_rgb(0.8, 0.8, 0.8);
        cr.set_font_size(24.0);
        cr.move_to(width as f64 / 2.0 - 150.0, height as f64 / 2.0);
        cr.show_text("No image data").unwrap();
    });
    
    projection1_box.append(&drawing1);
    projection1_frame.set_child(Some(&projection1_box));
    projections_container.append(&projection1_frame);
    drawing_areas.borrow_mut().push(drawing1.clone());
    
    // Second projection (if available)
    if study.series.len() > 1 {
        let projection2_frame = Frame::new(None);
        projection2_frame.add_css_class("card");
        let projection2_box = Box::new(Orientation::Vertical, 5);
        projection2_box.set_margin_top(10);
        projection2_box.set_margin_bottom(10);
        projection2_box.set_margin_start(10);
        projection2_box.set_margin_end(10);
        
        let projection2_label = Label::new(Some("Projection 2"));
        projection2_label.add_css_class("title-3");
        projection2_box.append(&projection2_label);
        
        let drawing2 = DrawingArea::new();
        drawing2.set_size_request(400, 500);
        drawing2.add_css_class("viewer");
        
        drawing2.set_draw_func(move |_, cr, width, height| {
            info!("Projection 2 drawing area redraw: {}x{}", width, height);
            cr.set_source_rgb(0.0, 0.0, 0.0);
            cr.paint().unwrap();
            if let Some(study) = current_study_clone2.borrow().as_ref() {
                if study.series.len() > 1 {
                    let series = &study.series[1];
                    if !series.instances.is_empty() {
                        let instance = &series.instances[0];
                        if let Some(image_data) = &instance.image_data {
                            info!("Projection 2: {}x{} pixels", image_data.width, image_data.height);
                            render_dicom_image(cr, image_data, width, height);
                            return;
                        }
                    }
                }
            }
            cr.set_source_rgb(0.8, 0.8, 0.8);
            cr.set_font_size(24.0);
            cr.move_to(width as f64 / 2.0 - 150.0, height as f64 / 2.0);
            cr.show_text("No image data").unwrap();
        });
        
        projection2_box.append(&drawing2);
        projection2_frame.set_child(Some(&projection2_box));
        projections_container.append(&projection2_frame);
        drawing_areas.borrow_mut().push(drawing2.clone());
    }
    
    // Third projection (if available)
    if study.series.len() > 2 {
        let projection3_frame = Frame::new(None);
        projection3_frame.add_css_class("card");
        let projection3_box = Box::new(Orientation::Vertical, 5);
        projection3_box.set_margin_top(10);
        projection3_box.set_margin_bottom(10);
        projection3_box.set_margin_start(10);
        projection3_box.set_margin_end(10);
        
        let projection3_label = Label::new(Some("Projection 3"));
        projection3_label.add_css_class("title-3");
        projection3_box.append(&projection3_label);
        
        let drawing3 = DrawingArea::new();
        drawing3.set_size_request(400, 500);
        drawing3.add_css_class("viewer");
        
        drawing3.set_draw_func(move |_, cr, width, height| {
            info!("Projection 3 drawing area redraw: {}x{}", width, height);
            cr.set_source_rgb(0.0, 0.0, 0.0);
            cr.paint().unwrap();
            if let Some(study) = current_study_clone3.borrow().as_ref() {
                if study.series.len() > 2 {
                    let series = &study.series[2];
                    if !series.instances.is_empty() {
                        let instance = &series.instances[0];
                        if let Some(image_data) = &instance.image_data {
                            info!("Projection 3: {}x{} pixels", image_data.width, image_data.height);
                            render_dicom_image(cr, image_data, width, height);
                            return;
                        }
                    }
                }
            }
            cr.set_source_rgb(0.8, 0.8, 0.8);
            cr.set_font_size(24.0);
            cr.move_to(width as f64 / 2.0 - 150.0, height as f64 / 2.0);
            cr.show_text("No image data").unwrap();
        });
        
        projection3_box.append(&drawing3);
        projection3_frame.set_child(Some(&projection3_box));
        projections_container.append(&projection3_frame);
        drawing_areas.borrow_mut().push(drawing3.clone());
    }
    
    viewer_content.append(&projections_container);
    main_box.append(&viewer_content);
}
