# SumMRIse

A cross-platform medical imaging viewer with AI analysis capabilities, designed to help patients view and understand their MRI and CT scan data from CDs provided by medical clinics.

## Features

- **Cross-platform Support**: Native GUI applications for Linux, macOS, and Windows
- **DICOM Support**: Full support for DICOM medical imaging files
- **3D Visualization**: Interactive 3D volume rendering with modern OpenGL
- **2D Slice Viewer**: Precise slice navigation and analysis
- **AI Analysis**: Integration with ChatGPT for second opinion analysis
- **Modern UI**: Clean, intuitive interface built with native GUI frameworks

## Architecture

The application is built with a modular architecture to support multiple platforms:

```
summrise/
├── crates/
│   ├── core/           # Core application logic and models
│   ├── dicom/          # DICOM file parsing and processing
│   ├── rendering/      # 3D/2D rendering engine (WGPU/OpenGL)
│   ├── llm/            # LLM integration (ChatGPT API)
│   ├── gui-linux/      # Linux GUI (GTK4)
│   ├── gui-macos/      # macOS GUI (Cocoa/Swift)
│   └── gui-windows/    # Windows GUI (WinUI/Win32)
└── src/                # Main application entry point
```

## Requirements

### Linux
- Rust 1.70+
- GTK4 development libraries
- OpenGL 3.3+ support

### Dependencies Installation

#### Ubuntu/Debian
```bash
sudo apt update
sudo apt install build-essential libgtk-4-dev libgl1-mesa-dev
```

#### Fedora/RHEL
```bash
sudo dnf install gcc gtk4-devel mesa-libGL-devel
```

#### Arch Linux
```bash
sudo pacman -S base-devel gtk4 mesa
```

## Building

1. Clone the repository:
```bash
git clone https://github.com/billiamdan/sumMRIse.git
cd sumMRIse
```

2. Build the application:
```bash
cargo build --release
```

3. Run the application:
```bash
cargo run --release
```

## Usage

1. **Launch the Application**: Start SumMRIse from your applications menu or command line
2. **Open DICOM Folder**: Click "Open Folder" and select the folder containing your DICOM files from the medical CD
3. **View 3D Data**: Use the 3D viewer to explore your medical imaging data in three dimensions
4. **Navigate Slices**: Use the 2D slice viewer to examine individual slices with precise controls
5. **AI Analysis**: Configure your ChatGPT API key in settings to get AI-powered analysis of your scans

## Configuration

The application stores configuration in:
- **Linux**: `~/.config/summrise/config.toml`
- **macOS**: `~/Library/Application Support/summrise/config.toml`
- **Windows**: `%APPDATA%/summrise/config.toml`

### LLM Configuration

To enable AI analysis, you'll need to:
1. Obtain an OpenAI API key
2. Open Settings in the application
3. Enter your API key and preferred model (default: gpt-4)
4. Save the configuration

## Medical Disclaimer

**IMPORTANT**: This application is designed for educational and informational purposes only. It is NOT a replacement for professional medical diagnosis or treatment. Always consult with qualified healthcare professionals for medical advice, diagnosis, and treatment decisions.

The AI analysis feature provides a "second opinion" for educational purposes and should never be used as the sole basis for medical decisions.

## Contributing

Contributions are welcome! Please feel free to submit pull requests or open issues for bugs and feature requests.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Roadmap

- [ ] macOS GUI implementation
- [ ] Windows GUI implementation
- [ ] Advanced 3D rendering features
- [ ] Measurement tools
- [ ] Export functionality
- [ ] Plugin system for additional analysis tools
- [ ] Cloud storage integration
- [ ] Multi-language support

## Support

For support, please open an issue on GitHub or contact the maintainers.

---

**SumMRIse** - Making medical imaging accessible and understandable for everyone.