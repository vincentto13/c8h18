//! Per-frame context passed to the front end and the race.
use crate::assets::Assets;
use crate::audio::Audio;
use crate::data::config::Config;
use crate::input::{Input, KeyPress};
use crate::pad::PadState;

pub struct Ctx<'a> {
    pub assets: &'a Assets,
    pub audio: &'a mut Audio,
    pub cfg: &'a mut Config,
    /// held keys
    pub input: &'a Input,
    /// gamepad state this frame (port addition)
    pub pad: &'a PadState,
    /// keys pressed since the last frame (consumers remove what they use)
    pub keys: Vec<KeyPress>,
    /// sound and music available (no -s)
    pub sound: bool,
}
