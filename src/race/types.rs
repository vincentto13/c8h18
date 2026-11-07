//! Race setup and result types shared with the front end.

#[derive(Clone, Debug)]
pub struct PlayerSetup {
    pub name: String,
    pub human: bool,
    pub team: u8,
    pub craft: u8,
    /// effective difficulty 0..3
    pub difficulty: u8,
}

#[derive(Clone, Debug)]
pub struct RaceSetup {
    /// track 1..6
    pub track: usize,
    pub laps: u32,
    /// slot 0..7 (slot 0 = human unless demo)
    pub players: Vec<PlayerSetup>,
    pub demo: bool,
    pub championship: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PlayerResult {
    pub name: String,
    pub team: u8,
    pub craft: u8,
    pub human: bool,
    /// 0 = not classified
    pub finish_place: u8,
    pub finish_time: i32,
    pub best_lap: i32,
    /// race laps (laps of the race distance)
    pub laps: u32,
    pub kills: i32,
    pub deaths: i32,
    pub hit_accuracy: i32,
    pub assessment: u8,
    pub points: i32,
}

#[derive(Clone, Debug, Default)]
pub struct RaceResult {
    pub demo: bool,
    pub championship: bool,
    pub track: usize,
    /// right Shift+Q: quit the program without results
    pub quit_game: bool,
    /// the human was classified (results screens are shown)
    pub classified: bool,
    pub players: Vec<PlayerResult>,
}

/// Race points by finish place 1..8.
pub const POINTS: [i32; 8] = [20, 16, 13, 11, 9, 7, 6, 5];
pub const AI_NAMES: [&str; 8] = ["SCOPER", "BARNSY", "SHUNTLY", "COPSE", "MANNY", "MCLALIN", "MAD", "ATROW"];
