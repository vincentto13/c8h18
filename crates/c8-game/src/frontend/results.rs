//! After a race: points, championship, records, high scores and the results screens (spec 02 §12),
//! plus the high-score table screen (§8).
use super::markup::{st, text_w, Mk};
use crate::assets::Assets;
use crate::data::config::{Config, DEFAULT_LAPS};
use crate::race::types::{RaceResult, AI_NAMES, POINTS};
use std::sync::Mutex;

/// Each driver's last assessment (used by QUIT CHAMPIONSHIP's high-score entry).
static LAST_ASSESS: Mutex<[u8; 8]> = Mutex::new([15; 8]);

pub enum Screen {
    Stats(Box<RaceResult>),
    /// title string, rows (name, value) in slot order
    Table(usize, Vec<(String, i32)>),
}

/// Driver names by slot for the championship (slot 0 = the human).
pub fn driver(cfg: &Config, slot: usize) -> (String, u8, u8) {
    let name = if slot == 0 { cfg.name() } else { AI_NAMES[slot].to_string() };
    let (t, c) = (cfg.team() as usize, cfg.craft() as usize);
    let (team, craft) = if slot == 0 { (t, c) } else { ((slot + t) % 8, (slot + c) % 6) };
    (name, team as u8, craft as u8)
}

/// Insert all 8 drivers into the high-score table with their championship points (§12.5).
pub fn enter_hiscores(cfg: &mut Config) {
    let la = *LAST_ASSESS.lock().unwrap();
    for slot in 0..8 {
        let (name, team, craft) = driver(cfg, slot);
        let score = cfg.champ_points(slot) as i32;
        let a = la[slot];
        let Some(row) = (0..50).find(|&r| {
            let (s, ra, ..) = cfg.hiscore(r);
            s < score || (s == score && ra > a)
        }) else {
            continue;
        };
        for r in (row + 1..50).rev() {
            let e = cfg.hiscore(r - 1);
            cfg.set_hiscore(r, &e);
        }
        cfg.set_hiscore(row, &(score, a, name, team, craft));
    }
}

/// Championship place of the human (1..8).
fn champ_place(cfg: &Config) -> usize {
    let me = cfg.champ_points(0);
    let mut place = 8;
    for k in 1..8 {
        let p = cfg.champ_points(k);
        if p < me || p == me {
            // equal points: ties go to the lower index (slot 0 beats everybody)
            place -= 1;
        }
    }
    place
}

pub struct Outcome {
    pub screens: Vec<Screen>,
    /// championship mode after the progression
    pub champ: bool,
}

/// Process a finished (non-demo) race.
pub fn after_race(res: RaceResult, cfg: &mut Config) -> Outcome {
    after_race_in(res, cfg, crate::tracks::available())
}

/// `after_race` with the available tracks given: a championship runs over those, in track order
/// (port addition; with all six this is the original progression).
fn after_race_in(mut res: RaceResult, cfg: &mut Config, avail: &[usize]) -> Outcome {
    let champ = res.championship;
    let mut screens = Vec::new();
    let rn = cfg.race_number() as usize;
    let track = if (1..=6).contains(&res.track) { res.track - 1 } else { rn };
    for p in res.players.iter_mut() {
        p.points = if (1..=8).contains(&p.finish_place) { POINTS[p.finish_place as usize - 1] } else { 0 };
    }
    {
        let mut la = LAST_ASSESS.lock().unwrap();
        for (i, p) in res.players.iter().enumerate().take(8) {
            if p.finish_place != 0 {
                la[i] = p.assessment;
            }
        }
    }
    if res.classified {
        for (i, p) in res.players.iter().enumerate().take(8) {
            cfg.set_champ_points(i, cfg.champ_points(i).saturating_add(p.points as i16));
        }
        screens.push(Screen::Stats(Box::new(res.clone())));
        if champ {
            // track records
            let max_race = DEFAULT_LAPS[track] as i32 * 9999;
            let mut best_lap: Option<(i32, String)> = None;
            let mut best_race: Option<(i32, String)> = None;
            for p in res.players.iter().filter(|p| p.finish_place != 0) {
                if p.best_lap > 0 && p.best_lap < 9999 && best_lap.as_ref().is_none_or(|b| p.best_lap < b.0) {
                    best_lap = Some((p.best_lap, p.name.clone()));
                }
                if p.finish_time > 0 && p.finish_time < max_race && best_race.as_ref().is_none_or(|b| p.finish_time < b.0) {
                    best_race = Some((p.finish_time, p.name.clone()));
                }
            }
            if let Some((t, n)) = best_lap {
                let (old, _) = cfg.best_lap(track);
                if old <= 0 || t < old {
                    cfg.set_best_lap(track, t, &n);
                }
            }
            if let Some((t, n)) = best_race {
                let (old, _) = cfg.best_race(track);
                if old <= 0 || t < old {
                    cfg.set_best_race(track, t, &n);
                }
            }
            let names: Vec<String> = (0..8).map(|k| res.players.get(k).map(|p| p.name.clone()).unwrap_or_else(|| driver(cfg, k).0)).collect();
            let pts: Vec<(String, i32)> = (0..res.players.len().min(8)).map(|k| (names[k].clone(), res.players[k].points)).collect();
            screens.push(Screen::Table(106, pts));
            let tot: Vec<(String, i32)> = (0..8).map(|k| (names[k].clone(), cfg.champ_points(k) as i32)).collect();
            screens.push(Screen::Table(139, tot));
        }
    }
    let mut champ_on = champ;
    if champ && res.classified {
        if let Some(next) = crate::tracks::next_in(avail, rn) {
            if champ_place(cfg) <= 3 {
                cfg.set_race_number(next as u16);
            } else {
                for (i, p) in res.players.iter().enumerate().take(8) {
                    cfg.set_champ_points(i, cfg.champ_points(i).saturating_sub(p.points as i16));
                }
            }
        } else {
            cfg.set_race_number(crate::tracks::first_in(avail).unwrap_or(0) as u16);
            champ_on = false;
            enter_hiscores(cfg);
        }
    }
    Outcome { screens, champ: champ_on }
}

fn num(n: i32) -> Vec<u8> {
    n.to_string().into_bytes()
}

/// STATISTICS (§12.3).
pub fn stats_markup(res: &RaceResult, cfg: &Config, a: &Assets) -> Vec<u8> {
    let l = cfg.language();
    let h2 = a.fonts[2].space_h() as i32;
    let h3 = a.fonts[3].space_h() as i32;
    let mut m = Mk::new();
    m.c('U', 2).c('C', 1).c('O', 0).c('Y', 24).c('J', 1).txt(&st(l, 137));
    let y1 = 8 + h2 + h3 + h3 + 2;
    let y2 = y1 + h3 + 2;
    m.c('U', 3).c('C', 1).c('O', 0);
    let name = |k: usize| -> Vec<u8> {
        match res.players.get(k) {
            Some(p) if p.finish_place != 0 => crate::data::strings::to_cp437(&p.name),
            _ => b" ".to_vec(),
        }
    };
    let centred_at = |m: &mut Mk, s: &[u8], cx: i32, y: i32| {
        let w = text_w(a, 3, s);
        m.c('J', 0).c('X', cx - w / 2).c('Y', y).txt(s);
    };
    m.c('J', 0).c('X', 10).c('Y', y1).txt(&name(0));
    for (k, cx) in [(2, 203), (4, 359), (6, 515)] {
        centred_at(&mut m, &name(k), cx, y1);
    }
    for (k, cx) in [(1, 125), (3, 281), (5, 437)] {
        centred_at(&mut m, &name(k), cx, y2);
    }
    let n7 = name(7);
    m.c('J', 0).c('X', 628 - text_w(a, 3, &n7)).c('Y', y2).txt(&n7);
    let r0 = y2 + h3 + 2;
    let heads = [111, 114, 113, 94, 40, 104, 112, 128];
    for (row, &hs) in heads.iter().enumerate() {
        let y = r0 + row as i32 * 2 * (h3 + 2);
        m.c('C', 184).c('O', 64).c('Y', y).c('J', 1).txt(&st(l, hs));
        m.c('C', 1).c('O', 0);
        for k in 0..8 {
            let Some(p) = res.players.get(k) else { continue };
            if p.finish_place == 0 {
                continue;
            }
            let v: Vec<u8> = match row {
                0 => format!("{}%", p.hit_accuracy).into_bytes(),
                1 => num(p.kills),
                2 => num(p.deaths),
                3 => num(if p.laps > 0 { p.finish_time / p.laps as i32 } else { p.finish_time }),
                4 => num(p.best_lap),
                5 => num(p.finish_time),
                6 => format!("{}/20", 20 - p.assessment as i32).into_bytes(),
                _ => num(p.finish_place as i32),
            };
            centred_at(&mut m, &v, 8 + 78 * k as i32 + 39, y + h3 + 2);
        }
    }
    if let Some(me) = res.players.iter().find(|p| p.human).or(res.players.first()) {
        let y = r0 + 7 * 2 * (h3 + 2) + 3 * (h3 + 2);
        m.c('U', 2).c('C', 1).c('O', 0).c('Y', y).c('J', 1).txt(&st(l, 149 + me.assessment.min(20) as usize));
    }
    m.0
}

/// POINTS / CHAMPIONSHIP TABLE (§12.4).
pub fn table_markup(title: usize, rows: &[(String, i32)], cfg: &Config, a: &Assets) -> Vec<u8> {
    let l = cfg.language();
    let h = a.fonts[2].space_h() as i32;
    let w = a.fonts[2].space_w() as i32;
    let mut m = Mk::new();
    m.c('U', 2).c('C', 184).c('O', 64).c('Y', h * 4 / 3 + 32).c('J', 1).txt(&st(l, title));
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by(|&x, &y| rows[y].1.cmp(&rows[x].1).then(x.cmp(&y)));
    for (r, &i) in order.iter().enumerate() {
        let r = r as i32;
        let y = 32 + (r + 3) * h + (r + 1) * (h / 2);
        if r < 3 {
            m.c('C', 1).c('O', 0);
        } else {
            m.c('C', 184).c('O', 64);
        }
        m.c('J', 0).c('Y', y).c('X', 32 + 2 * w).s(&rows[i].0);
        m.c('X', 608 - 5 * w).txt(&num(rows[i].1));
    }
    m.0
}

/// High-score table (§8).
pub fn hiscore_markup(cfg: &Config, a: &Assets) -> Vec<u8> {
    let l = cfg.language();
    let h = a.fonts[3].space_h() as i32;
    let w = a.fonts[3].space_w() as i32;
    let mut m = Mk::new();
    m.c('U', 3).c('C', 184).c('O', 64).c('Y', h + 32).c('J', 1).txt(&st(l, 38));
    for i in 0..19 {
        let (score, ass, name, ..) = cfg.hiscore(i);
        let y = (i as i32 + 3) * h + 32;
        m.c('J', 0).c('Y', y).c('X', w + 32).s(&name);
        m.c('J', 1).txt(&num(score));
        let t = st(l, 149 + (ass as usize).min(20));
        m.c('J', 0).c('X', 608 - w - text_w(a, 3, &t)).txt(&t);
    }
    m.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::types::PlayerResult;

    /// A classified championship race on the config's current track, finished in slot order
    /// (the human wins) or with the human last.
    fn race(cfg: &Config, human_wins: bool) -> RaceResult {
        let rn = cfg.race_number() as usize;
        let players = (0..8)
            .map(|k| PlayerResult {
                name: format!("P{k}"),
                team: k as u8,
                craft: (k % 6) as u8,
                human: k == 0,
                finish_place: if human_wins { k as u8 + 1 } else { [8, 1, 2, 3, 4, 5, 6, 7][k] },
                finish_time: 9000,
                best_lap: 700,
                laps: DEFAULT_LAPS[rn] as u32,
                kills: 0,
                deaths: 0,
                hit_accuracy: 0,
                assessment: 0,
                points: 0,
            })
            .collect();
        RaceResult { demo: false, championship: true, track: rn + 1, quit_game: false, classified: true, players }
    }

    /// Race a whole championship (human always wins) from the first track; the tracks raced.
    fn run(avail: &[usize]) -> Vec<usize> {
        let mut cfg = Config::fresh();
        cfg.set_race_number(crate::tracks::first_in(avail).unwrap() as u16);
        let mut raced = Vec::new();
        loop {
            raced.push(cfg.race_number() as usize);
            let out = after_race_in(race(&cfg, true), &mut cfg, avail);
            if !out.champ {
                break;
            }
            assert!(raced.len() < 10);
        }
        assert_eq!(cfg.race_number() as usize, crate::tracks::first_in(avail).unwrap(), "back at the first track");
        raced
    }

    #[test]
    fn championship_over_the_available_tracks() {
        assert_eq!(run(&[0, 1, 2, 3, 4, 5]), vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(run(&[0]), vec![0]);
        assert_eq!(run(&[1]), vec![1]);
        assert_eq!(run(&[0, 2, 5]), vec![0, 2, 5]);
    }

    #[test]
    fn losing_retakes_the_same_track() {
        let avail = [0, 3];
        let mut cfg = Config::fresh();
        cfg.set_race_number(0);
        let out = after_race_in(race(&cfg, false), &mut cfg, &avail);
        assert!(out.champ);
        assert_eq!(cfg.race_number(), 0);
        assert_eq!(cfg.champ_points(0), 0, "points taken back");
        let out = after_race_in(race(&cfg, true), &mut cfg, &avail);
        assert!(out.champ);
        assert_eq!(cfg.race_number(), 3);
    }
}
