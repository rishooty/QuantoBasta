// This implementation is based on the guide provided by [RetroGameDeveloper/RetroReversing].
// Original guide can be found at [https://www.retroreversing.com/CreateALibRetroFrontEndInRust].
// Copyright (c) 2023 Nicholas Ricciuti
//
// video.rs
//
// This module handles video output for the emulator, including pixel format conversions,
// rendering frames, and interfacing with the libretro video callbacks.

use crate::{libretro::EmulatorState, VideoData, PIXEL_FORMAT_CHANNEL, VIDEO_DATA_CHANNEL};
use libretro_sys::PixelFormat;
use pixels::Pixels;
use winit::event_loop::ControlFlow;

// Represents the pixel format used by the emulator.
pub struct EmulatorPixelFormat(pub PixelFormat);

// Provides a default pixel format for the emulator.
impl Default for EmulatorPixelFormat {
    fn default() -> Self {
        EmulatorPixelFormat(PixelFormat::ARGB8888)
    }
}

#[cfg(target_os = "windows")]
pub fn check_vrr_status() {
    print!("do that windows reg thing")
}

#[cfg(target_os = "linux")]
pub fn check_vrr_status() {
    print!("do that linux x11 or wayland thing")
}

pub fn is_vrr_ready(monitor: &winit::monitor::MonitorHandle, original_framerate: f64) -> bool {
    let mut min_refresh_rate = f64::MAX;
    let mut max_refresh_rate = f64::MIN;
    let mut count_not_divisible_by_five = 0;

    for video_mode in monitor.video_modes() {
        let refresh_rate = video_mode.refresh_rate_millihertz() as f64 / 1000.0;
        if refresh_rate < min_refresh_rate {
            min_refresh_rate = refresh_rate;
        }
        if refresh_rate > max_refresh_rate {
            max_refresh_rate = refresh_rate;
        }

        // Check if refresh rate is not divisible by 5 and is not 144Hz
        if refresh_rate % 5.0 != 0.0 && refresh_rate.round() as i32 != 144 {
            count_not_divisible_by_five += 1;
        }
    }

    println!(
        "Min and Max refresh rates for monitor '{}': {}Hz, {}Hz",
        monitor.name().unwrap(),
        min_refresh_rate,
        max_refresh_rate
    );

    return count_not_divisible_by_five > 1
        && min_refresh_rate <= original_framerate
        && original_framerate <= max_refresh_rate;
}

// Callback function that the libretro core will use to pass video frame data.
pub unsafe extern "C" fn libretro_set_video_refresh_callback(
    frame_buffer_data: *const libc::c_void,
    width: libc::c_uint,
    height: libc::c_uint,
    pitch: libc::size_t,
) {
    if frame_buffer_data.is_null() {
        println!("frame_buffer_data was null");
        return;
    }

    let length_of_frame_buffer = (pitch as u32) * height;
    let buffer_slice = std::slice::from_raw_parts(
        frame_buffer_data as *const u8,
        length_of_frame_buffer as usize,
    );

    // Here, we just pass the raw frame buffer data without converting it
    let video_data = VideoData {
        frame_buffer: buffer_slice.to_vec(),
        pitch: pitch as u32,
    };

    if let Err(e) = VIDEO_DATA_CHANNEL.0.send(video_data) {
        eprintln!("Failed to send video data: {:?}", e);
        // Handle error appropriately
    }
}

// Sets up the pixel format for the emulator based on the libretro core's specifications.
pub fn set_up_pixel_format() -> u8 {
    let mut bpp = 2 as u8;

    let pixel_format_receiver = &PIXEL_FORMAT_CHANNEL.1.lock().unwrap();

    for pixel_format in pixel_format_receiver.try_iter() {
        bpp = match pixel_format {
            PixelFormat::ARGB1555 | PixelFormat::RGB565 => 2,
            PixelFormat::ARGB8888 => 4,
        };
        println!("Core will send us pixel data in format {:?}", pixel_format);
    }

    bpp
}

pub fn render_frame(
    pixels: &mut Pixels,
    current_state: &EmulatorState,
    video_height: u32,
    video_width: u32,
) -> ControlFlow {
    let mut rgb565_to_rgb8888_table: [u32; 65536] = [0; 65536];
    for i in 0..65536 {
        let r = (i >> 11) & 0x1F;
        let g = (i >> 5) & 0x3F;
        let b = i & 0x1F;

        let r = ((r * 527 + 23) >> 6) as u32;
        let g = ((g * 259 + 33) >> 6) as u32;
        let b = ((b * 527 + 23) >> 6) as u32;

        rgb565_to_rgb8888_table[i] = 0xFF000000 | (r << 16) | (g << 8) | b;
    }

    let mut argb1555_to_argb8888_table: [u32; 32768] = [0; 32768];
    for i in 0..32768 {
        let a = (i >> 15) & 0x01;
        let r = (i >> 10) & 0x1F;
        let g = (i >> 5) & 0x1F;
        let b = i & 0x1F;

        let a = (a * 255) as u32;
        let r = ((r * 527 + 23) >> 6) as u32;
        let g = ((g * 527 + 23) >> 6) as u32;
        let b = ((b * 527 + 23) >> 6) as u32;

        argb1555_to_argb8888_table[i] = (a << 24) | (r << 16) | (g << 8) | b;
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
                if source_index >= video_data.frame_buffer.len() || dest_index >= frame.len() {
                    break;
                }

                match current_state.pixel_format.0 {
                    PixelFormat::RGB565 => {
                        // Convert RGB565 to ARGB8888
                        let first_byte = video_data.frame_buffer[source_index];
                        let second_byte = video_data.frame_buffer[source_index + 1];
                        let rgb565 = (first_byte as u16) | ((second_byte as u16) << 8);

                        // Look up the converted pixel in the table
                        let argb8888 = rgb565_to_rgb8888_table[rgb565 as usize];

                        // Copy the converted pixel into the frame buffer
                        frame[dest_index..dest_index + 4].copy_from_slice(&argb8888.to_ne_bytes());
                    }
                    PixelFormat::ARGB1555 => {
                        // Convert ARGB1555 to ARGB8888
                        let first_byte = video_data.frame_buffer[source_index];
                        let second_byte = video_data.frame_buffer[source_index + 1];
                        let argb1555 = (first_byte as u16) | ((second_byte as u16) << 8);

                        // Look up the converted pixel in the table
                        let argb8888 = argb1555_to_argb8888_table[argb1555 as usize];

                        // Copy the converted pixel into the frame buffer
                        frame[dest_index..dest_index + 4].copy_from_slice(&argb8888.to_ne_bytes());
                    }
                    PixelFormat::ARGB8888 => {
                        // Directly copy ARGB8888 pixel
                        let source_slice = &video_data.frame_buffer[source_index..source_index + 4];
                        frame[dest_index..dest_index + 4].copy_from_slice(source_slice);
                    }
                }
            }
        }

        // Render the frame buffer
        if pixels.render().is_err() {
            return ControlFlow::Exit;
        }
    }
    return ControlFlow::Poll;
}
