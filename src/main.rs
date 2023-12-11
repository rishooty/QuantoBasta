// This implementation is based on the guide provided by [RetroGameDeveloper/RetroReversing].
// Original guide can be found at [https://www.retroreversing.com/CreateALibRetroFrontEndInRust].
// Copyright (c) 2023 Nicholas Ricciuti

// Import necessary modules from other files and crates
mod audio;
mod input;
mod libretro;
mod video;
//use gilrs::{Event as gEvent, GamepadId, Gilrs};
pub static AUDIO_CONDVAR: Condvar = Condvar::new();
use crate::audio::AUDIO_BUFFER;
use libretro_sys::PixelFormat;
use once_cell::sync::Lazy;
use pixels::wgpu::PresentMode;
use pixels::PixelsBuilder;
use pixels::SurfaceTexture;
use rodio::{OutputStream, Sink};
use std::process;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Condvar;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use std::time::Instant;
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
static FINAL_SAMPLE_RATE: AtomicU32 = AtomicU32::new(0);

// Structure to hold video data
struct VideoData {
    frame_buffer: Vec<u8>,
    pitch: u32,
}

// The main function, entry point of the application
fn main() {
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

    // Auto refresh setup
    let primary_monitor = event_loop.primary_monitor().unwrap();
    let monitor_refresh_rate_mhz = primary_monitor.refresh_rate_millihertz().unwrap();
    let monitor_refresh_rate_hz = monitor_refresh_rate_mhz as f64 / 1000.0;
    let original_framerate = av_info.as_ref().map_or(60.0, |av_info| av_info.timing.fps);
    let is_vrr_ready = video::is_vrr_ready(&primary_monitor, original_framerate);

    let mut target_fps = monitor_refresh_rate_hz;
    if is_vrr_ready {
        target_fps = original_framerate;
    }
    let swap_interval = (monitor_refresh_rate_hz / original_framerate).round();
    let vsync_sample_factor = monitor_refresh_rate_hz / original_framerate;

    let window = WindowBuilder::new()
        .with_title("Retro Emulator")
        .with_inner_size(LogicalSize::new(video_width, video_height))
        .build(&event_loop)
        .unwrap();
    let window_id: winit::window::WindowId = window.id();

    // use winit::window::Fullscreen;
    // // Assume `window` is the `winit` window that `pixels` is rendering to.

    let physical_width = video_width;
    let physical_height = video_height;

    let pixels_build_result = PixelsBuilder::new(
        physical_width,
        physical_height,
        SurfaceTexture::new(physical_width, physical_height, &window),
    )
    .present_mode(PresentMode::AutoVsync)
    .build();

    let mut pixels = pixels_build_result.unwrap();

    // Extract the audio sample rate from the emulator state
    let sample_rate = av_info.as_ref().map_or(0.0, |av_info| {
        av_info.timing.sample_rate * vsync_sample_factor
    });
    FINAL_SAMPLE_RATE.store(sample_rate as u32, Ordering::SeqCst);

    let _audio_thread = thread::spawn(move || {
        println!("Audio Thread Started");
        let (_stream, stream_handle) = OutputStream::try_default().unwrap();
        let sink = Sink::try_new(&stream_handle).unwrap();
        loop {
            // Try to lock the buffer pool
            if let Ok(buffer) = AUDIO_BUFFER.try_lock() {
                // Wait for the Condvar with a timeout 
                // of 16ms per swap interval
                let (buffer, _timeout_result) = AUDIO_CONDVAR
                    .wait_timeout(
                        buffer,
                        Duration::from_millis(16.0 as u64 * swap_interval as u64),
                    )
                    .unwrap();
                unsafe {
                    audio::play_audio(&sink, &buffer, sample_rate as u32);
                }
                AUDIO_CONDVAR.notify_all();
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

    // TODO, IMPLEMENT IN AUDIO THREAD
    let frame_duration = Duration::from_secs_f64(swap_interval / target_fps); // for 60 FPS

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
                    &primary_monitor,
                    &mut is_fullscreen,
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
                last_update = Instant::now();

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
                let _guard = AUDIO_BUFFER.lock().unwrap();
                *control_flow =
                    video::render_frame(&mut pixels, &current_state, video_height, video_width);
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
