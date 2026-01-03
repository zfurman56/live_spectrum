# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
# Development build with fast iteration (uses dynamic linking)
cargo run --features bevy/dynamic_linking

# Release build
cargo run --release

# Check code without building
cargo check

# Format and lint
cargo fmt
cargo clippy
```

The Cargo.toml configures debug builds with `opt-level = 1` for local code and `opt-level = 3` for dependencies, balancing compile time with runtime performance.

## Architecture

This is a real-time audio spectrum analyzer built with Bevy (game engine) that captures microphone input and displays a live frequency graph.

### Data Flow Pipeline

The application uses Bevy's ECS (Entity Component System) with four main systems that run in sequence:

1. **setup_mic()** - Exclusive system that initializes cpal microphone input and creates a thread-safe channel for audio samples
2. **mic_input()** - Buffers samples, applies Hanning window, computes FFT via spectrum-analyzer, outputs raw spectrum
3. **envelope_spectrum()** - Applies exponential moving average filter for smoother visualization
4. **animate_spectra()** - Renders the spectrum as a line graph each frame

### Key Components and Resources

- `Spectrum([f32; SPECTRUM_SIZE])` - Frequency bin data storage
- `RawSpectrum` / `EnvelopeSpectrum` - Tag components to distinguish spectrum types in queries
- `MicData` - Channel receiver for audio samples (wrapped in Mutex for thread safety)
- `SampleBuffer` - Accumulates samples until we have enough for an FFT
- `MicStream` - Non-send resource holding the cpal audio stream

### Important Constants

- `FFT_SIZE = 2048` - FFT window size (must be power of 2)
- `SPECTRUM_SIZE = FFT_SIZE / 2` - Number of frequency bins displayed
- `ENVELOPE_FILTER_CONST = 0.95` - Smoothing factor (higher = smoother, slower response)
- `PLOT_WIDTH = 800.0` - Graph width in pixels

### Dependencies

- **bevy 0.15** - Game engine framework for rendering and ECS
- **bevy_prototype_lyon 0.13** - Vector graphics for drawing the spectrum line
- **cpal 0.15** - Cross-platform audio input
- **spectrum-analyzer** - FFT and frequency spectrum computation
