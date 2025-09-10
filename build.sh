#!/bin/bash

# SumMRIse Build Script
# This script builds the application for different platforms

set -e

echo "🏥 SumMRIse - Medical Imaging Viewer Build Script"
echo "=================================================="

# Check if Rust is installed
if ! command -v cargo &> /dev/null; then
    echo "❌ Rust is not installed. Please install Rust from https://rustup.rs/"
    exit 1
fi

# Check Rust version
RUST_VERSION=$(cargo --version | cut -d' ' -f2)
echo "✅ Rust version: $RUST_VERSION"

# Detect platform
PLATFORM=$(uname -s)
echo "🖥️  Platform: $PLATFORM"

# Install dependencies based on platform
case $PLATFORM in
    "Linux")
        echo "📦 Checking Linux dependencies..."
        
        # Check for GTK4
        if ! pkg-config --exists gtk4; then
            echo "❌ GTK4 not found. Installing dependencies..."
            
            if command -v apt &> /dev/null; then
                echo "Installing via apt..."
                sudo apt update
                sudo apt install -y build-essential libgtk-4-dev libgl1-mesa-dev pkg-config
            elif command -v dnf &> /dev/null; then
                echo "Installing via dnf..."
                sudo dnf install -y gcc gtk4-devel mesa-libGL-devel pkgconfig
            elif command -v pacman &> /dev/null; then
                echo "Installing via pacman..."
                sudo pacman -S --needed base-devel gtk4 mesa pkgconf
            else
                echo "❌ Package manager not supported. Please install GTK4 development libraries manually."
                exit 1
            fi
        else
            echo "✅ GTK4 found"
        fi
        
        # Check for OpenGL
        if ! pkg-config --exists gl; then
            echo "❌ OpenGL development libraries not found"
            exit 1
        else
            echo "✅ OpenGL found"
        fi
        ;;
    "Darwin")
        echo "🍎 macOS detected"
        echo "📦 Checking macOS dependencies..."
        
        if ! command -v brew &> /dev/null; then
            echo "❌ Homebrew not found. Please install Homebrew from https://brew.sh/"
            exit 1
        fi
        
        # Install dependencies via Homebrew
        brew install gtk4 pkg-config
        ;;
    "MINGW"*|"CYGWIN"*|"MSYS"*)
        echo "🪟 Windows detected"
        echo "📦 Windows dependencies should be handled by vcpkg or manual installation"
        ;;
    *)
        echo "❌ Unsupported platform: $PLATFORM"
        exit 1
        ;;
esac

# Build the application
echo "🔨 Building SumMRIse..."

# Clean previous build
echo "🧹 Cleaning previous build..."
cargo clean

# Build in release mode
echo "⚡ Building in release mode..."
cargo build --release

if [ $? -eq 0 ]; then
    echo "✅ Build successful!"
    echo "📁 Binary location: target/release/summrise"
    
    # Show binary size
    if [ -f "target/release/summrise" ]; then
        SIZE=$(du -h target/release/summrise | cut -f1)
        echo "📊 Binary size: $SIZE"
    fi
    
    echo ""
    echo "🚀 To run the application:"
    echo "   cargo run --release"
    echo ""
    echo "📖 For more information, see README.md"
else
    echo "❌ Build failed!"
    exit 1
fi
