// This implementation is based on the guide provided by [RetroGameDeveloper/RetroReversing].
// Original guide can be found at [https://www.retroreversing.com/CreateALibRetroFrontEndInRust].
// Copyright (c) 2023 Nicholas Ricciuti
//
// The `audio` module handles audio processing and playback for the emulator.
// It uses the `rodio` crate for audio output and integrates with the libretro API for audio data.

use crate::{AUDIO_DATA_CHANNEL, TARGET_FPS};
use once_cell::sync::Lazy;
use rodio::buffer::SamplesBuffer;
use rodio::Sink;
use std::{
    collections::VecDeque,
    sync::{atomic::Ordering, Arc, Mutex},
    thread,
    time::Duration,
};

// Constants for audio processing.
const AUDIO_CHANNELS: usize = 2; // Stereo audio with left and right channels.
const FINAL_SAMPLE_RATE: u32 = 48_000; // Sample rate in Hertz (48 kHz).
const BUFFER_DURATION_MS: u32 = 64; // Duration of each audio buffer in milliseconds.
const BUFFER_LENGTH: usize = (FINAL_SAMPLE_RATE as u32 * BUFFER_DURATION_MS / 1000) as usize; // Number of samples in each buffer.
const POOL_SIZE: usize = 20; // Number of buffers in the audio buffer pool.

// Represents an audio buffer containing raw audio samples.
pub struct AudioBuffer {
    data: Vec<i16>, // Vector to store the 16-bit audio samples.
}

impl AudioBuffer {
    // Constructs a new `AudioBuffer` with a specified size.
    pub fn new(size: usize) -> Self {
        AudioBuffer {
            data: vec![0; size],
        }
    }

    // Clears the buffer, removing all audio samples.
    pub fn clear(&mut self) {
        self.data.clear();
    }

    // Extends the buffer with audio samples from a slice.
    pub fn extend_from_slice(&mut self, slice: &[i16]) {
        self.data.extend_from_slice(slice);
    }

    // Returns a pointer to the audio data.
    pub fn as_ptr(&self) -> *const i16 {
        self.data.as_ptr()
    }

    // Returns the length of the audio data in samples.
    pub fn len(&self) -> usize {
        self.data.len()
    }
}

// Global buffer pool for managing audio buffers.
pub static BUFFER_POOL: Lazy<Mutex<Vec<Arc<Mutex<VecDeque<i16>>>>>> = Lazy::new(|| {
    let mut pool = Vec::new();
    for _ in 0..POOL_SIZE {
        pool.push(Arc::new(Mutex::new(VecDeque::with_capacity(BUFFER_LENGTH))));
    }
    Mutex::new(pool)
});

// Plays audio using the `rodio` library.
pub unsafe fn play_audio(sink: &Sink, audio_samples: &mut VecDeque<i16>, sample_rate: u32) {
    audio_samples.make_contiguous();
    let audio_slices = audio_samples.as_slices();
    let audio_slice = audio_slices.0; // You might need to handle the case when there are two slices.
    let source = SamplesBuffer::new(AUDIO_CHANNELS.try_into().unwrap(), sample_rate, audio_slice);
    sink.append(source);
}


// Callback function for the libretro API to handle individual audio samples.
pub unsafe extern "C" fn libretro_set_audio_sample_callback(left: i16, right: i16) {
    println!("libretro_set_audio_sample_callback");
}

pub unsafe extern "C" fn libretro_set_audio_sample_batch_callback(
    audio_data: *const i16,
    frames: libc::size_t,
) -> libc::size_t {
    let buffer_arc: Arc<Mutex<VecDeque<i16>>>;
    let mut pool = BUFFER_POOL.lock().unwrap();
    buffer_arc = pool
        .pop()
        .unwrap_or_else(|| Arc::new(Mutex::new(VecDeque::with_capacity(BUFFER_LENGTH))));

    {
        let mut buffer = buffer_arc.lock().unwrap();
        let audio_slice = std::slice::from_raw_parts(audio_data, frames * AUDIO_CHANNELS);

        // If the buffer is full, discard the oldest data to make room for the new data.
        while buffer.len() + audio_slice.len() > buffer.capacity() {
            buffer.pop_front();
        }

        // Add the new audio data to the buffer.
        buffer.extend(audio_slice.iter().copied());
    }

    // Return the buffer to the pool.
    pool.push(buffer_arc);

    frames
}
