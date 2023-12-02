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
use pixels::Pixels;
use pixels::SurfaceTexture;
use rodio::{OutputStream, Sink};
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use video::convert_rgb565_to_xrgb8888;
use winit::dpi::LogicalSize;
use winit::event::{Event, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;

// Define global static variables for handling input, pixel format, video, and audio data
static BUTTONS_PRESSED: Lazy<Mutex<(Vec<i16>, Vec<i16>)>> =
    Lazy::new(|| Mutex::new((vec![0; 16], vec![0; 16])));
static BYTES_PER_PIXEL: AtomicU8 = AtomicU8::new(4); // Default value for bytes per pixel
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

    let event_loop = EventLoop::new();

    let window = WindowBuilder::new()
        .with_title("Retro Emulator")
        .with_inner_size(LogicalSize::new(video_width, video_height))
        .build(&event_loop)
        .unwrap();
    let window_id: winit::window::WindowId = window.id();

    let physical_width = video_width;
    let physical_height = video_height;

    let surface_texture = SurfaceTexture::new(physical_width, physical_height, &window);
    let mut pixels = Pixels::new(physical_width, physical_height, surface_texture).unwrap();

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
    // let config = libretro::setup_config().unwrap();
    // let key_device_map = input::key_device_map(&config);
    // let joypad_device_map = input::setup_joypad_device_map(&config);
    // let mut gilrs = Gilrs::new().unwrap(); // Initialize gamepad handling
    // let mut active_gamepad: Option<GamepadId> = None;

    // Main application loop
    event_loop.run(move |event, _, control_flow| {
        match event {
            Event::WindowEvent {
                event:
                    WindowEvent::Resized(new_inner_size)
                    | WindowEvent::ScaleFactorChanged {
                        new_inner_size: &mut new_inner_size,
                        ..
                    },
                window_id: id,
            } if id == window_id => {
                let new_physical_width = new_inner_size.width;
                let new_physical_height = new_inner_size.height;

                let _ =
                    pixels.resize_surface(new_physical_width as u32, new_physical_height as u32);
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id: id,
                ..
            } if id == window_id => *control_flow = ControlFlow::Exit,
            Event::RedrawRequested(id) if id == window_id => {
                // Render your emulator frame here
                unsafe {
                    let core_api = &core.lock().unwrap().api;
                    (core_api.retro_run)();
                }
                // If needed, set up pixel format
                if current_state.bytes_per_pixel == 0 {
                    let pixel_format_receiver = &PIXEL_FORMAT_CHANNEL.1.lock().unwrap();

                    for pixel_format in pixel_format_receiver.try_iter() {
                        current_state.pixel_format.0 = pixel_format;
                        let bpp = match pixel_format {
                            PixelFormat::ARGB1555 | PixelFormat::RGB565 => 2,
                            PixelFormat::ARGB8888 => 4,
                        };
                        println!("Core will send us pixel data in format {:?}", pixel_format);
                        BYTES_PER_PIXEL.store(bpp, Ordering::SeqCst);
                        current_state.bytes_per_pixel = bpp;
                    }
                }

                // Copy the emulator frame data to the `pixels` frame
                let video_data_receiver = VIDEO_DATA_CHANNEL.1.lock().unwrap();

                // Iterate over the video data received from the core
                for video_data in video_data_receiver.try_iter() {
                    // Extract the video data dimensions
                    let pitch = video_data.pitch as usize; // number of bytes per row

                    // Get the pixels frame buffer
                    let frame = pixels.frame_mut();

                    // Assuming `current_state.pixel_format.0` gives you the source format...
                    let bytes_per_pixel_source = current_state.bytes_per_pixel as usize;

                    for y in 0..video_height as usize {
                        for x in 0..(video_width as usize) {
                            let source_index = y * pitch + x * bytes_per_pixel_source;
                            let dest_index = (y * video_width as usize + x) * 4; // 4 bytes per pixel for ARGB8888

                            // Ensure we're not going out of bounds
                            if source_index >= video_data.frame_buffer.len()
                                || dest_index >= frame.len()
                            {
                                break;
                            }

                            match current_state.pixel_format.0 {
                                PixelFormat::RGB565 => {
                                    // Convert RGB565 to ARGB8888
                                    let first_byte = video_data.frame_buffer[source_index];
                                    let second_byte = video_data.frame_buffer[source_index + 1];
                                    let argb_color =
                                        convert_rgb565_to_xrgb8888(first_byte, second_byte);

                                    // Copy the converted pixel into the frame buffer
                                    frame[dest_index..dest_index + 4]
                                        .copy_from_slice(&argb_color.to_ne_bytes());
                                }
                                PixelFormat::ARGB8888 => {
                                    // Directly copy ARGB8888 pixel
                                    let source_slice =
                                        &video_data.frame_buffer[source_index..source_index + 4];
                                    frame[dest_index..dest_index + 4].copy_from_slice(source_slice);
                                }
                                // Handle other source formats as needed
                                _ => { /* Handle other pixel formats if necessary */ }
                            }
                        }
                    }

                    // Render the frame buffer
                    if pixels.render().is_err() {
                        *control_flow = ControlFlow::Exit;
                        return;
                    }
                }

                // Request a redraw for the next frame
                window.request_redraw();
            }
            _ => (),
        }
    });
}

// Old Input handling Example
////////////////////////////////////////////////////////////////////
//     while window.is_open() && !window.is_key_down(Key::Escape) {
//         {
//             let mut buttons = BUTTONS_PRESSED.lock().unwrap();
//             let buttons_pressed = &mut buttons.0;
//             let mut game_pad_active: bool = false;

//             while let Some(gEvent { id, .. }) = gilrs.next_event() {
//                 // println!("{:?} New event from {}: {:?}", time, id, event);
//                 active_gamepad = Some(id);
//             }

//             // Handle gamepad and keyboard input
//             if let Some(gamepad) = active_gamepad {
//                 input::handle_gamepad_input(
//                     &joypad_device_map,
//                     &gilrs,
//                     &Some(gamepad),
//                     buttons_pressed,
//                 );
//                 game_pad_active = true;
//             }
//             input::handle_keyboard_input(
//                 core_api,
//                 &window,
//                 &mut current_state,
//                 buttons_pressed,
//                 &key_device_map,
//                 &config,
//                 game_pad_active,
//             );
//         }
//         // graphics processing...
//     }
//}
