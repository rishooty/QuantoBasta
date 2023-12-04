// This implementation is based on the guide provided by [RetroGameDeveloper/RetroReversing].
// Original guide can be found at [https://www.retroreversing.com/CreateALibRetroFrontEndInRust].
// Copyright (c) 2023 Nicholas Ricciuti
//
// video.rs
//
// This module handles video output for the emulator, including pixel format conversions,
// rendering frames, and interfacing with the libretro video callbacks.

use libretro_sys::PixelFormat;
use std::sync::atomic::Ordering;

use crate::{libretro::EmulatorState, VideoData, PIXEL_FORMAT_CHANNEL, VIDEO_DATA_CHANNEL};

// Represents the pixel format used by the emulator.
pub struct EmulatorPixelFormat(pub PixelFormat);

// Provides a default pixel format for the emulator.
impl Default for EmulatorPixelFormat {
    fn default() -> Self {
        EmulatorPixelFormat(PixelFormat::ARGB8888)
    }
}

pub fn convert_rgb565_to_xrgb8888(first_byte: u8, second_byte: u8) -> u32 {
    // Extract the color components from the 16-bit RGB565 format
    let red = (first_byte & 0b1111_1000) >> 3;
    let green = ((first_byte & 0b0000_0111) << 3) | ((second_byte & 0b1110_0000) >> 5);
    let blue = second_byte & 0b0001_1111;

    // Scale up the color components to fit in the 32-bit XRGB8888 format
    // RGB565 has 5 bits for R and B, and 6 bits for G, so we need to scale them up
    let red = (red << 3) | (red >> 2); // 5-bit to 8-bit
    let green = (green << 2) | (green >> 4); // 6-bit to 8-bit
    let blue = (blue << 3) | (blue >> 2); // 5-bit to 8-bit

    // Combine the color components into one 32-bit XRGB8888 value
    // XRGB8888 format: 0xFFRRGGBB, where FF is the alpha channel set to fully opaque
    0xFF000000 | ((red as u32) << 16) | ((green as u32) << 8) | (blue as u32)
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