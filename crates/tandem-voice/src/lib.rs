//! Dictate and Conductor: audio capture, hotkeys, local speech-to-text,
//! insertion and the realtime voice client (ARCHITECTURE §2.10, §12). Built in M8.

pub mod audio;
pub mod capture;
#[cfg(feature = "cpal")]
pub mod capture_cpal;
pub mod chimes;
pub mod cloud;
pub mod devices;
pub mod dictate;
pub mod dictionary;
pub mod guards;
pub mod hotkey;
pub mod insert;
pub mod media;
pub mod mic_owner;
pub mod models;
pub mod postproc;
pub mod stt;
pub mod take;
pub mod wake;
