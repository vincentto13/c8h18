//! Audio data and synthesis shared by the game and the editor (spec 01 §11, spec 09): the bank
//! containers SOUND.DAT and MUSIC.DAT (read and write), WAV samples, XMIDI tunes and the music
//! player on an emulated OPL3. Output devices and mixing stay with each program.
pub mod container;
pub mod music;
pub mod ogg;
pub mod opl3;
pub mod smf;
pub mod tune;
pub mod wav;
pub mod xmidi;
