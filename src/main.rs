// This implementation is based on the guide provided by [RetroGameDeveloper/RetroReversing].
// Original guide can be found at [https://www.retroreversing.com/CreateALibRetroFrontEndInRust].
// Copyright (c) 2023 Nicholas Ricciuti

// Import necessary modules from other files and crates
mod audio;
mod input;
mod libretro;
mod video;
use audio::AudioBuffer;
//use gilrs::{Event as gEvent, GamepadId, Gilrs};
use libretro_sys::PixelFormat;
use once_cell::sync::Lazy;
use pixels::wgpu::PresentMode;
use pixels::PixelsBuilder;
use pixels::SurfaceTexture;
use rodio::{OutputStream, Sink};
use std::process;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::time::Instant;
use video::Color;
use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

// Define global static variables for handling input, pixel format, video, and audio data
static BUTTONS_PRESSED: Lazy<Mutex<(Vec<i16>, Vec<i16>)>> =
    Lazy::new(|| Mutex::new((vec![0; 16], vec![0; 16])));
static PIXEL_FORMAT_CHANNEL: Lazy<(Sender<PixelFormat>, Arc<Mutex<Receiver<PixelFormat>>>)> =
    Lazy::new(|| {
        let (sender, receiver) = channel::<PixelFormat>();
        (sender, Arc::new(Mutex::new(receiver)))
    });
static VIDEO_DATA_CHANNEL: Lazy<(Sender<VideoData>, Arc<Mutex<Receiver<VideoData>>>)> =
    Lazy::new(|| {
        let (sender, receiver) = channel::<VideoData>();
        (sender, Arc::new(Mutex::new(receiver)))
    });
static AUDIO_DATA_CHANNEL: Lazy<(
    Sender<Arc<Mutex<AudioBuffer>>>,
    Arc<Mutex<Receiver<Arc<Mutex<AudioBuffer>>>>>,
)> = Lazy::new(|| {
    let (sender, receiver) = channel::<Arc<Mutex<AudioBuffer>>>();
    (sender, Arc::new(Mutex::new(receiver)))
});

// Structure to hold video data
struct VideoData {
    frame_buffer: Vec<u8>,
    pitch: u32,
}

// The main function, entry point of the application
fn main() {
    //video::check_vrr_status();
    //process::exit(0);

    // Parse command line arguments to get ROM and library names
    let (rom_name, library_name) = libretro::parse_command_line_arguments();
    // Initialize emulator state with default values
    let mut current_state = libretro::EmulatorState {
        rom_name,
        library_name,
        current_save_slot: 0,
        av_info: None,
        pixel_format: video::EmulatorPixelFormat(PixelFormat::ARGB8888),
        bytes_per_pixel: 0,
    };

    // Initialize the core of the emulator and update the emulator state
    let (core, updated_state) = libretro::Core::new(current_state);
    let core = Arc::new(Mutex::new(core));
    current_state = updated_state;
    let av_info = &current_state.av_info;
    let video_width = (av_info.as_ref().unwrap().geometry).base_width;
    let video_height = (av_info.as_ref().unwrap().geometry).base_height;
    let mut is_fullscreen = false;
    let event_loop = EventLoop::new();

    // Auto refresh setup WIP
    let primary_monitor = event_loop.primary_monitor().unwrap();
    let monitor_refresh_rate_mhz = primary_monitor.refresh_rate_millihertz().unwrap();
    let monitor_refresh_rate_hz = monitor_refresh_rate_mhz as f64 / 1000.0;
    let original_framerate = av_info.as_ref().map_or(60.0, |av_info| av_info.timing.fps);
    let is_vrr_ready = video::is_vrr_ready(&primary_monitor, original_framerate);

    let mut bfi_factor: f64 = 0.0;
    let mut target_fps = monitor_refresh_rate_hz;
    let mut present_mode: PresentMode = PresentMode::AutoVsync;
    if is_vrr_ready {
        target_fps = original_framerate;
        present_mode = PresentMode::AutoNoVsync;
    } else {
        bfi_factor = (monitor_refresh_rate_hz / original_framerate - 1.0).round();
    }

    let window = WindowBuilder::new()
        .with_title("Retro Emulator")
        .with_inner_size(LogicalSize::new(video_width, video_height))
        .build(&event_loop)
        .unwrap();
    let window_id: winit::window::WindowId = window.id();

    // use winit::window::Fullscreen;
    // // Assume `window` is the `winit` window that `pixels` is rendering to.
    // window.set_fullscreen(Some(Fullscreen::Borderless(None)));

    let physical_width = video_width;
    let physical_height = video_height;

    let pixels_build_result = PixelsBuilder::new(
        physical_width,
        physical_height,
        SurfaceTexture::new(physical_width, physical_height, &window),
    )
    .present_mode(present_mode)
    .build();

    let mut pixels = pixels_build_result.unwrap();

    // Extract the audio sample rate from the emulator state
    let sample_rate = av_info
        .as_ref()
        .map_or(0.0, |av_info| av_info.timing.sample_rate);

    // Spawn a new thread for audio handling
    let _audio_thread = thread::spawn(move || {
        println!("Audio Thread Started");
        let (_stream, stream_handle) = OutputStream::try_default().unwrap();
        let sink = Sink::try_new(&stream_handle).unwrap();
        loop {
            let receiver = AUDIO_DATA_CHANNEL.1.lock().unwrap();
            // Play audio in a loop
            for buffer_arc in receiver.try_iter() {
                let buffer = buffer_arc.lock().unwrap();
                unsafe {
                    audio::play_audio(&sink, &*buffer, sample_rate as u32);
                }
            }
        }
    });

    // Set up libretro callbacks for video, input, and audio
    unsafe {
        let core_api = &core.lock().unwrap().api;
        (core_api.retro_init)();
        (core_api.retro_set_video_refresh)(video::libretro_set_video_refresh_callback);
        (core_api.retro_set_input_poll)(input::libretro_set_input_poll_callback);
        (core_api.retro_set_input_state)(input::libretro_set_input_state_callback);
        (core_api.retro_set_audio_sample)(audio::libretro_set_audio_sample_callback);
        (core_api.retro_set_audio_sample_batch)(audio::libretro_set_audio_sample_batch_callback);
        println!("About to load ROM: {}", &current_state.rom_name);
        // Load the ROM file
        libretro::load_rom_file(core_api, &current_state.rom_name);
    }

    // Prepare configurations for input handling
    let config = libretro::setup_config().unwrap();
    let key_device_map = input::key_device_map(&config);
    // let joypad_device_map = input::setup_joypad_device_map(&config);
    // let mut gilrs = Gilrs::new().unwrap(); // Initialize gamepad handling
    // let mut active_gamepad: Option<GamepadId> = None;

    // Main application loop
    let mut last_update = Instant::now();
    let frame_duration = Duration::from_secs_f64(1.0 / target_fps); // for 60 FPS
    let mut color_frame_counter: u64 = 0;
    let mut most_common_color: Color = Color::ColorU16(0x0000);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(last_update + frame_duration);
        match event {
            Event::WindowEvent {
                event: WindowEvent::KeyboardInput { input, .. },
                ..
            } => {
                let mut buttons = BUTTONS_PRESSED.lock().unwrap();
                let buttons_pressed = &mut buttons.0;

                input::handle_keyboard_input(
                    input,
                    buttons_pressed,
                    &key_device_map,
                    &window,
                    is_fullscreen,
                );
            }
            Event::WindowEvent {
                event,
                window_id: id,
            } if id == window_id => {
                let new_inner_size = match event {
                    WindowEvent::Resized(new_inner_size) => new_inner_size,
                    WindowEvent::ScaleFactorChanged { new_inner_size, .. } => *new_inner_size,
                    WindowEvent::Moved(_) => window.inner_size(),
                    _ => return,
                };

                let new_physical_width = new_inner_size.width;
                let new_physical_height = new_inner_size.height;

                let _ =
                    pixels.resize_surface(new_physical_width as u32, new_physical_height as u32);
                //handle refresh set
                //handle audio set
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id: id,
                ..
            } if id == window_id => *control_flow = ControlFlow::Exit,
            Event::MainEventsCleared => {
                if current_state.bytes_per_pixel != 0
                    && bfi_factor > 0.0
                    && color_frame_counter < bfi_factor.round() as u64
                {
                    // Render a greyscale frame
                    match most_common_color {
                        Color::ColorU16(color) => match current_state.pixel_format.0 {
                            PixelFormat::RGB565 => {
                                *control_flow =
                                    video::render_color_frame_rgb565(&mut pixels, color);
                            }
                            PixelFormat::ARGB1555 => {
                                *control_flow =
                                    video::render_color_frame_argb1555(&mut pixels, color);
                            }
                            PixelFormat::ARGB8888 => {}
                        },
                        Color::ColorU32(color) => {
                            // color is a u32 value
                            *control_flow = video::render_color_frame_argb8888(&mut pixels, color);
                        }
                    }
                    // Increment the greyscale frame counter
                    color_frame_counter += 1;
                } else {
                    // Render your emulator frame here
                    unsafe {
                        let core_api = &core.lock().unwrap().api;
                        (core_api.retro_run)();
                    }
                    // If needed, set up pixel format
                    if current_state.bytes_per_pixel == 0 {
                        (current_state.bytes_per_pixel, current_state.pixel_format) =
                            video::set_up_pixel_format();
                    }
                    (*control_flow, most_common_color) = video::render_frame(
                        &mut pixels,
                        &current_state,
                        video_height,
                        video_width,
                        bfi_factor > 0.0,
                    );

                    // Reset the greyscale frame counter
                    color_frame_counter = 0;
                }

                last_update = Instant::now();
            }

            _ => (),
        }
    });
}

// Old Input handling Example
////////////////////////////////////////////////////////////////////
// while window.is_open() && !window.is_key_down(Key::Escape) {
//     {
//         let mut buttons = BUTTONS_PRESSED.lock().unwrap();
//         let buttons_pressed = &mut buttons.0;
//         let mut game_pad_active: bool = false;

//         while let Some(gEvent { id, .. }) = gilrs.next_event() {
//             // println!("{:?} New event from {}: {:?}", time, id, event);
//             active_gamepad = Some(id);
//         }

//         // Handle gamepad and keyboard input
//         if let Some(gamepad) = active_gamepad {
//             input::handle_gamepad_input(
//                 &joypad_device_map,
//                 &gilrs,
//                 &Some(gamepad),
//                 buttons_pressed,
//             );
//             game_pad_active = true;
//         }
//         input::handle_keyboard_input(
//             core_api,
//             &window,
//             &mut current_state,
//             buttons_pressed,
//             &key_device_map,
//             &config,
//             game_pad_active,
//         );
//     }
//     // graphics processing...
// }
//}
