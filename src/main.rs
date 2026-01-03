use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use spectrum_analyzer::{samples_fft_to_spectrum, FrequencyLimit};
use spectrum_analyzer::scaling::divide_by_N_sqrt;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};


const FFT_SIZE: usize = 2048; // Must be power of 2
const SPECTRUM_SIZE: usize = FFT_SIZE / 2;
const MAX_DISPLAY_FREQ: f32 = 6000.0; // Only show frequencies up to this (Hz)
const AMPLITUDE_SCALE: f32 = 500.0; // Vertical scaling for the spectrum

const ENVELOPE_FILTER_CONST: f32 = 0.95;

const PLOT_WIDTH: f32 = 800.0;
const PLOT_Y_ZERO: f32 = -50.0;

#[derive(Component)]
struct Spectrum([f32; SPECTRUM_SIZE]);
#[derive(Component)]
struct RawSpectrum;
#[derive(Component)]
struct EnvelopeSpectrum;

#[derive(Resource)]
struct MicSampleRate(u32);
#[derive(Resource)]
struct MicData(Arc<Mutex<Receiver<f32>>>);
#[derive(Resource)]
struct SampleBuffer(Vec<f32>);
#[derive(Resource)]
struct MaxDisplayBin(usize); // Number of bins to display (based on MAX_DISPLAY_FREQ)

// cpal::Stream is not Send, so we store it as a non-send resource
struct MicStream(#[allow(dead_code)] cpal::Stream);


fn main() {
    App::new()
        .insert_resource(ClearColor(Color::WHITE))
        .add_plugins(DefaultPlugins)
        .add_plugins(ShapePlugin)
        .add_systems(Startup, (setup_mic, setup_spectra, draw_scale).chain())
        .add_systems(Update, (mic_input, envelope_spectrum, animate_spectra).chain())
        .add_systems(Update, close_on_esc)
        .run();
}

fn close_on_esc(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut exit: EventWriter<AppExit>,
) {
    if keyboard.just_pressed(KeyCode::Escape) {
        exit.send(AppExit::Success);
    }
}

// Setup gathering of microphone data
// We use an exclusive system to insert non-send resources
fn setup_mic(world: &mut World) {
    // Use channel to send data from the mic callback thread back to our worker threads
    let (tx, rx) = channel();

    let host = cpal::default_host();
    let device = host.default_input_device().expect("No microphone found");

    let config = device
        .default_input_config()
        .expect("No supported mic config");

    // Save the sample rate so we can use it to find frequencies later
    let sample_rate = config.sample_rate();

    let stream = device.build_input_stream(
        &config.into(),
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            for val in data {
                let _ = tx.send(*val);
            }
        },
        move |err| {
            eprintln!("Audio stream error: {}", err);
        },
        None,
    ).unwrap();
    stream.play().unwrap();

    world.insert_non_send_resource(MicStream(stream));
    world.insert_resource(MicSampleRate(sample_rate.0));
    world.insert_resource(MicData(Arc::new(Mutex::new(rx))));
    world.insert_resource(SampleBuffer(Vec::with_capacity(FFT_SIZE)));

    // Calculate how many FFT bins correspond to MAX_DISPLAY_FREQ
    let nyquist = sample_rate.0 as f32 / 2.0;
    let bin_width = nyquist / SPECTRUM_SIZE as f32;
    let max_bin = ((MAX_DISPLAY_FREQ / bin_width) as usize).min(SPECTRUM_SIZE);
    world.insert_resource(MaxDisplayBin(max_bin));
}

// Setup the spectra we have and the paths we'll use for associated graphs
fn setup_spectra(mut commands: Commands) {
    commands.spawn(Camera2d);

    commands.spawn((Spectrum([0.0; SPECTRUM_SIZE]), RawSpectrum));

    let path = PathBuilder::new().build();
    commands.spawn((
        ShapeBundle {
            path,
            transform: Transform::default(),
            ..default()
        },
        Stroke::new(Color::BLACK, 1.0),
        Spectrum([0.0; SPECTRUM_SIZE]),
        EnvelopeSpectrum,
    ));
}

// Take our microphone data and get frequency information from it using FFT
fn mic_input(
    mut query: Query<&mut Spectrum, With<RawSpectrum>>,
    mut sample_buffer: ResMut<SampleBuffer>,
    mic_data: Res<MicData>,
    sample_rate: Res<MicSampleRate>,
) {
    let mut spectrum = query.single_mut();

    // Collect new samples from mic
    let new_samples: Vec<f32> = mic_data.0.lock().unwrap().try_iter().collect();
    sample_buffer.0.extend(new_samples);

    // Process when we have enough samples
    while sample_buffer.0.len() >= FFT_SIZE {
        // Take the first FFT_SIZE samples
        let samples: Vec<f32> = sample_buffer.0.drain(..FFT_SIZE).collect();

        // Apply Hanning window
        let windowed: Vec<f32> = samples.iter().enumerate().map(|(i, &s)| {
            let window = 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos());
            s * window
        }).collect();

        // Compute FFT
        if let Ok(freq_spectrum) = samples_fft_to_spectrum(
            &windowed,
            sample_rate.0,
            FrequencyLimit::All,
            Some(&divide_by_N_sqrt),
        ) {
            // Copy spectrum data to our component
            for (i, (_, val)) in freq_spectrum.data().iter().enumerate() {
                if i < SPECTRUM_SIZE {
                    spectrum.0[i] = val.val();
                }
            }
        }
    }
}

// Filter the raw spectrum from the microphone
fn envelope_spectrum(
    mic_query: Query<&Spectrum, (With<RawSpectrum>, Without<EnvelopeSpectrum>)>,
    mut envelope_query: Query<&mut Spectrum, With<EnvelopeSpectrum>>
) {
    let mic = mic_query.single();
    let mut envelope = envelope_query.single_mut();

    for i in 0..envelope.0.len() {
        envelope.0[i] =
            (envelope.0[i]*ENVELOPE_FILTER_CONST + mic.0[i]*(1.0-ENVELOPE_FILTER_CONST)).max(mic.0[i]);
    }
}

// Draw the scale for our graph
fn draw_scale(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
) {
    let width = PLOT_WIDTH / 2.0;
    let height = PLOT_Y_ZERO - 30.0;
    let num_ticks = 20;

    let color = Color::srgb(0.5, 0.5, 0.5);
    let font = asset_server.load("fonts/EBGaramond-Medium.ttf");

    // Line containing tick marks
    let mut path_builder = PathBuilder::new();
    path_builder.move_to(Vec2::new(-width, height));
    path_builder.line_to(Vec2::new(width, height));
    let path = path_builder.build();
    commands.spawn((
        ShapeBundle {
            path,
            transform: Transform::default(),
            ..default()
        },
        Stroke::new(color, 1.0),
    ));

    // Hz label
    commands.spawn((
        Text2d::new("Hz"),
        TextFont {
            font: font.clone(),
            font_size: 12.0,
            ..default()
        },
        TextColor(color),
        Transform::from_translation(Vec3::new(-width - 20.0, height, 0.0)),
    ));

    for i in 0..=num_ticks {
        let tick_pos = -width + (((i as f32) / (num_ticks as f32)) * width * 2.0);

        // Draw tick marks
        let mut path_builder = PathBuilder::new();
        path_builder.move_to(Vec2::new(tick_pos, height+10.0));
        path_builder.line_to(Vec2::new(tick_pos, height-10.0));
        let path = path_builder.build();
        commands.spawn((
            ShapeBundle {
                path,
                transform: Transform::default(),
                ..default()
            },
            Stroke::new(color, 1.0),
        ));

        // Draw labels - scale goes from 0 to MAX_DISPLAY_FREQ
        let freq_hz = ((i as f32) / (num_ticks as f32)) * MAX_DISPLAY_FREQ;
        commands.spawn((
            Text2d::new(format!("{:.0}", freq_hz)),
            TextFont {
                font: font.clone(),
                font_size: 12.0,
                ..default()
            },
            TextColor(color),
            Transform::from_translation(Vec3::new(tick_pos, height-20.0, 0.0)),
        ));
    }
}

// Actually draw the graph for each frame
fn animate_spectra(
    mut query: Query<(&mut Path, &Spectrum)>,
    max_bin: Res<MaxDisplayBin>,
) {
    for (mut path, spectrum) in query.iter_mut() {
        let mut path_builder = PathBuilder::new();

        let width = PLOT_WIDTH / 2.0;
        let samples = max_bin.0;

        for i in 0..samples {
            let height = spectrum.0[i] * AMPLITUDE_SCALE + PLOT_Y_ZERO;
            path_builder.line_to(Vec2::new(-width+((i as f32) / (samples as f32))*width*2.0, height));
        }
        *path = path_builder.build();
    }
}
