use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use stft::{STFT, WindowType};
use cpal::Stream;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc::{channel, Sender, Receiver};
use std::sync::{Arc, Mutex};


const OUT_SIZE: usize = 2048;

#[derive(Component)]
struct Spectrum([f32; OUT_SIZE]);
#[derive(Component)]
struct MicSpectrum;
#[derive(Component)]
struct EnvelopeSpectrum;

struct MicData(Arc<Mutex<Receiver<f32>>>);

fn main() {
    let (tx, rx) = channel();
    // If _stream is dropped, the input stream closes
    let _stream = setup_mic(tx);

    App::new()
        .insert_resource(ClearColor(Color::rgb(1.0, 1.0, 1.0)))
        .insert_resource(Msaa { samples: 4 })
        .insert_resource(STFT::<f32>::new(WindowType::Hanning, 2*OUT_SIZE, 1024))
        .insert_resource(MicData(Arc::new(Mutex::new(rx))))
        .add_plugins(DefaultPlugins)
        .add_plugin(ShapePlugin)
        .add_startup_system(setup_system)
        .add_system(mic_input)
        .add_system(envelope_spectrum)
        .add_system(animate_spectra)
        .add_system(bevy::input::system::exit_on_esc_system)
        .run();
}

fn setup_system(mut commands: Commands) {
    commands.spawn_bundle(OrthographicCameraBundle::new_2d());

    commands.spawn_bundle(GeometryBuilder::build_as(
        &PathBuilder::new().build(),
        DrawMode::Stroke(StrokeMode::new(Color::SILVER, 1.0)),
        Transform::default(),
    )).insert(Spectrum([0.0; OUT_SIZE])).insert(MicSpectrum);

    commands.spawn_bundle(GeometryBuilder::build_as(
        &PathBuilder::new().build(),
        DrawMode::Stroke(StrokeMode::new(Color::BLACK, 1.0)),
        Transform::default(),
    )).insert(Spectrum([0.0; OUT_SIZE])).insert(EnvelopeSpectrum);
}

fn setup_mic(tx: Sender<f32>) -> Stream {
    let host = cpal::default_host();
    let device = host.default_input_device().expect("No microphone found");

    let config = device
        .default_input_config()
        .expect("No supported mic config");

    let stream = device.build_input_stream(
        &config.into(),
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            for val in data {
                tx.send(*val).unwrap();
            }
        },
        move |_| {},
    ).unwrap();
    stream.play().unwrap();

    stream
}

fn animate_spectra(mut query: Query<(&mut Path, &Spectrum)>) {
    for (mut path, spectrum) in query.iter_mut() {
        let mut path_builder = PathBuilder::new();

        let width = 400.0;
        let samples = spectrum.0.len()/2;

        for i in 0..samples {
            let height = (spectrum.0[i/2] as f32)*100.0 - 50.0;
            path_builder.line_to(Vec2::new(-width+((i as f32) / (samples as f32))*width*2.0, height));
        }
        *path = path_builder.build();
    }
}

fn envelope_spectrum(
    mic_query: Query<&Spectrum, (With<MicSpectrum>, Without<EnvelopeSpectrum>)>,
    mut envelope_query: Query<&mut Spectrum, With<EnvelopeSpectrum>>
) {
    let mic = mic_query.single();
    let mut envelope = envelope_query.single_mut();

    for i in 0..envelope.0.len() {
        envelope.0[i] = (envelope.0[i]*0.95 + mic.0[i]*0.05).max(mic.0[i]);
    }
}

fn mic_input(
    mut query: Query<&mut Spectrum, With<MicSpectrum>>,
    mut stft: ResMut<STFT::<f32>>,
    mic_data: Res<MicData>
) {
    let mut spectrum = query.single_mut();
    let data: Vec<f32> = mic_data.0.lock().unwrap().try_iter().collect();
    stft.append_samples(&data);

    while stft.contains_enough_to_compute() {
        stft.compute_column(&mut spectrum.0[..]);
        // throw away data if it wasn't read by animate_spectrum fast enough
        stft.move_to_next_column();
    }
}

