use bevy::prelude::*;
use bevy_prototype_lyon::prelude::*;
use stft::{STFT, WindowType};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};


const DFT_OUT_SIZE: usize = 2048;
const MAX_DFT_BIN: usize = DFT_OUT_SIZE/2;
const DFT_STEP_SIZE: usize = 1024;
const ENVELOPE_FILTER_CONST: f32 = 0.95;
const PLOT_WIDTH: f32 = 800.0;
const PLOT_Y_ZERO: f32 = -50.0;

#[derive(Component)]
struct Spectrum([f32; DFT_OUT_SIZE]);
#[derive(Component)]
struct RawSpectrum;
#[derive(Component)]
struct EnvelopeSpectrum;

struct MicSampleRate(u32);
struct MicData(Arc<Mutex<Receiver<f32>>>);

fn main() {
    App::new()
        .insert_resource(ClearColor(Color::rgb(1.0, 1.0, 1.0)))
        .insert_resource(Msaa { samples: 4 })
        .add_plugins(DefaultPlugins)
        .add_plugin(ShapePlugin)
        .add_startup_system(setup_mic.exclusive_system())
        .add_startup_system(setup_spectra)
        .add_startup_system(draw_scale)
        .add_system(mic_input)
        .add_system(envelope_spectrum)
        .add_system(animate_spectra)
        .add_system(bevy::input::system::exit_on_esc_system)
        .run();
}

fn setup_spectra(mut commands: Commands) {
    commands.spawn_bundle(OrthographicCameraBundle::new_2d());

    commands.spawn().insert(Spectrum([0.0; DFT_OUT_SIZE])).insert(RawSpectrum);

    commands.spawn_bundle(GeometryBuilder::build_as(
        &PathBuilder::new().build(),
        DrawMode::Stroke(StrokeMode::new(Color::BLACK, 1.0)),
        Transform::default(),
    )).insert(Spectrum([0.0; DFT_OUT_SIZE])).insert(EnvelopeSpectrum);
}

fn setup_mic(world: &mut World) {
    let (tx, rx) = channel();

    let host = cpal::default_host();
    let device = host.default_input_device().expect("No microphone found");

    let config = device
        .default_input_config()
        .expect("No supported mic config");
    let sample_rate = config.sample_rate();

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

    world.insert_non_send_resource(stream);
    world.insert_resource(MicSampleRate(sample_rate.0));
    world.insert_resource(MicData(Arc::new(Mutex::new(rx))));
    world.insert_resource(STFT::<f32>::new(WindowType::Hanning, 2*DFT_OUT_SIZE, DFT_STEP_SIZE));

}

fn draw_scale(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    sample_rate: Res<MicSampleRate>
) {
    let mut paths = Vec::new();
    let mut labels = Vec::new();

    let mut path_builder = PathBuilder::new();

    let width = PLOT_WIDTH / 2.0;
    let height = PLOT_Y_ZERO - 30.0;

    // Line containing tick marks
    path_builder.move_to(Vec2::new(-width, height));
    path_builder.line_to(Vec2::new(width, height));
    paths.push(path_builder.build());

    let font = asset_server.load("fonts/EBGaramond-Medium.ttf");
    let text_style = TextStyle {
        font,
        font_size: 12.0,
        color: Color::GRAY,
    };
    let text_alignment = TextAlignment {
        vertical: VerticalAlign::Center,
        horizontal: HorizontalAlign::Center,
    };

    let num_ticks = 20;
    for i in 0..=num_ticks {
        let tick_pos = -width + (((i as f32) / (num_ticks as f32)) * width * 2.0);

        let mut path_builder = PathBuilder::new();
        path_builder.move_to(Vec2::new(tick_pos, height+10.0));
        path_builder.line_to(Vec2::new(tick_pos, height-10.0));
        paths.push(path_builder.build());

        let freq_hz = ((i as f32) / (num_ticks as f32))
            * ((MAX_DFT_BIN as f32) / (2.0 * DFT_OUT_SIZE as f32))
            * (sample_rate.0 as f32);
        labels.push((format!("{:.0}", freq_hz), Vec3::new(tick_pos, height-20.0, 0.0)));
    }

    for path in paths.iter() {
        commands.spawn_bundle(GeometryBuilder::build_as(
            path,
            DrawMode::Stroke(StrokeMode::new(Color::GRAY, 1.0)),
            Transform::default(),
        ));
    }

    for (text, pos) in labels {
        commands.spawn_bundle(Text2dBundle {
            text: Text::with_section(text, text_style.clone(), text_alignment),
            transform: Transform::from_translation(pos),
            ..default()
        });
    }

}

fn animate_spectra(mut query: Query<(&mut Path, &Spectrum)>) {
    for (mut path, spectrum) in query.iter_mut() {
        let mut path_builder = PathBuilder::new();

        let width = PLOT_WIDTH / 2.0;
        let samples = MAX_DFT_BIN;

        for i in 0..samples {
            let height = (spectrum.0[i] as f32)*100.0 + PLOT_Y_ZERO;
            path_builder.line_to(Vec2::new(-width+((i as f32) / (samples as f32))*width*2.0, height));
        }
        *path = path_builder.build();
    }
}

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

fn mic_input(
    mut query: Query<&mut Spectrum, With<RawSpectrum>>,
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

