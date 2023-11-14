use crate::CURRENT_EMULATOR_STATE;
use std::sync::mpsc::Sender;
use rodio::Sink;
use rodio::buffer::SamplesBuffer;
const AUDIO_CHANNELS: usize = 2; // left and right

pub unsafe fn play_audio( sink: &Sink, audio_samples: &Vec<i16>, sample_rate: u32) {
    if sink.empty() {
        let audio_slice = std::slice::from_raw_parts(audio_samples.as_ptr() as *const i16, audio_samples.len());
        let source = SamplesBuffer::new(2, sample_rate, audio_slice);
        sink.append(source);
        sink.play();
        sink.sleep_until_end();
    }
}

pub unsafe extern "C" fn libretro_set_audio_sample_callback(left: i16, right: i16) {
    println!("libretro_set_audio_sample_callback");
}

pub unsafe extern "C" fn libretro_set_audio_sample_batch_callback(
    audio_data: *const i16,
    frames: libc::size_t,
) -> libc::size_t {
    let audio_slice = std::slice::from_raw_parts(audio_data, frames * AUDIO_CHANNELS);
    CURRENT_EMULATOR_STATE.audio_data = Some(audio_slice.to_vec());
    return frames;
}

pub unsafe fn send_audio_to_thread(sender: &Sender<&Vec<i16>>) {
    // Send the audio samples to the audio thread using the channel
    match &CURRENT_EMULATOR_STATE.audio_data {
        Some(data) => {
            sender.send(data).unwrap();
        },
        None => {},
    }; 
}