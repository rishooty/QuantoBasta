// This implementation is based on the guide provided by [RetroGameDeveloper/RetroReversing].
// Original guide can be found at [https://www.retroreversing.com/CreateALibRetroFrontEndInRust].
// Copyright (c) 2023 Nicholas Ricciuti
//
// The `audio` module handles audio processing and playback for the emulator.
// It uses the `rodio` crate for audio output and integrates with the libretro API for audio data.

use once_cell::sync::Lazy;
use rodio::Sink;
use soundtouch::SoundTouch;
use std::sync::atomic::Ordering;
use std::sync::Mutex;

use crate::FINAL_SAMPLE_RATE;

// Constants for audio processing.
const AUDIO_CHANNELS: usize = 2; // Stereo audio with left and right channels.
const BUFFER_DURATION_MS: u32 = 64; // Duration of each audio buffer in milliseconds.

// Global buffer for managing audio data.
pub static AUDIO_BUFFER: Lazy<Mutex<Vec<i16>>> = Lazy::new(|| {
    let sample_rate: u32 = FINAL_SAMPLE_RATE.load(Ordering::SeqCst);
    let buffer_length: usize = (sample_rate * BUFFER_DURATION_MS / 1000) as usize;
    Mutex::new(vec![0; buffer_length])
});

// Plays audio using the `rodio` library.
pub unsafe fn play_audio(
    sink: &Sink,
    audio_samples: &Vec<i16>,
    sample_rate: u32,
    soundtouch: &mut SoundTouch,
) {
    // Convert the i16 samples to f32 for SoundTouch
    let audio_samples_f32: Vec<f32> = audio_samples.iter().map(|&sample| sample as f32).collect();

    // Feed the audio samples into SoundTouch
    soundtouch.put_samples(&audio_samples_f32, audio_samples_f32.len() / 2);

    // Retrieve the processed audio from SoundTouch
    let mut processed_samples: Vec<f32> = Vec::new();
    let mut buffer: [f32; 1024] = [0.0; 1024];
    let buffer_len = buffer.len();
    let mut n_samples = 1;
    while n_samples != 0 {
        n_samples = soundtouch.receive_samples(&mut buffer, buffer_len / 2);
        processed_samples.extend_from_slice(&buffer[0..n_samples]);
    }

    // Convert the f32 samples back to i16 for Rodio
    let processed_samples_i16: Vec<i16> = processed_samples
        .iter()
        .map(|&sample| sample as i16)
        .collect();

    // Play the processed audio with Rodio
    let source = rodio::buffer::SamplesBuffer::new(
        AUDIO_CHANNELS.try_into().unwrap(),
        sample_rate,
        &processed_samples_i16[..],
    );
    sink.append(source);
}

// Callback function for the libretro API to handle individual audio samples.
pub unsafe extern "C" fn libretro_set_audio_sample_callback(left: i16, right: i16) {
    println!("libretro_set_audio_sample_callback");
}

// In your callback function
pub unsafe extern "C" fn libretro_set_audio_sample_batch_callback(
    audio_data: *const i16,
    frames: libc::size_t,
) -> libc::size_t {
    let mut buffer = AUDIO_BUFFER.lock().unwrap();
    let audio_slice = std::slice::from_raw_parts(audio_data, frames * AUDIO_CHANNELS);

    // If the buffer is full, discard the oldest data to make room for the new data.
    while buffer.len() + audio_slice.len() > buffer.capacity() {
        buffer.remove(0);
    }

    // Add the new audio data to the buffer.
    buffer.extend(audio_slice.iter().copied());

    frames
}
