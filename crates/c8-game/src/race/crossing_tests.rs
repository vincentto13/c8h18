//! Port addition: crossings (a road over itself, spec 01 §9.7) on the example level built in code
//! by `c8_data::level::crossing_example`: the lower road along row 39 at height 0, the upper road
//! along column 59 over a deck with its top at 2 and its underside at 1.
use super::*;
use crate::data::level::crossing_example;

/// The crossing's core: where the two roads overlap.
fn in_core(p: [f32; 3]) -> bool {
    (57.5..62.5).contains(&p[0]) && (37.5..42.5).contains(&p[1])
}

fn world() -> World {
    World::new(crossing_example(), Vec::new())
}

fn craft(w: &mut World, x: f32, y: f32, z: Option<f32>, yaw: f32, human: bool) -> usize {
    let mut c = Craft::default();
    c.active = true;
    c.slot = w.crafts.len();
    c.human = human;
    c.difficulty = 2;
    c.personality = PERSONALITY[c.slot % 8];
    c.pos = [x, y, 0.0];
    c.yaw = yaw;
    c.race_laps = 20;
    w.init_craft(&mut c);
    if let Some(z) = z {
        // on a given surface instead of the top one
        c.pos[2] = z + 128.0 * U;
        c.prev_pos = c.pos;
        for s in &mut c.sensors {
            *s = (c.pos[2], 0.0);
        }
    }
    c.started = true;
    c.lap = 1;
    w.crafts.push(c);
    w.crafts.len() - 1
}

fn drive(w: &mut World, i: usize, ticks: u32) -> Vec<[f32; 3]> {
    let mut path = Vec::new();
    for _ in 0..ticks {
        w.frame += 1;
        w.update_craft(i, HumanIn { accel: true, ..Default::default() });
        path.push(w.crafts[i].pos);
    }
    path
}

#[test]
fn the_example_is_layered() {
    let w = world();
    assert!(w.track.layered);
    // the waypoints on and under the deck share a cell but are two waypoints, on their own levels
    let at: Vec<&super::super::track::Waypoint> = w.track.wps.iter().filter(|p| p.x == 59.5 && p.y == 39.5).collect();
    assert_eq!(at.len(), 2);
    let mut floors: Vec<f32> = at.iter().map(|p| p.floor).collect();
    floors.sort_by(f32::total_cmp);
    assert_eq!(floors, [0.0, 2.0]);
}

/// A craft on the deck stays on it; a craft under it passes through, below the underside.
#[test]
fn deck_carries_and_road_passes_under() {
    let mut w = world();
    // upper road, from the north, heading south: up the ramp, over the deck, down the other ramp
    let i = craft(&mut w, 59.5, 20.5, None, 32768.0, true);
    let path = drive(&mut w, i, 200);
    let core: Vec<f32> = path.iter().filter(|p| in_core(**p)).map(|p| p[2]).collect();
    assert!(core.len() > 3, "the craft never crossed: {:?}", path.last());
    assert!(core.iter().all(|&z| z > 2.2 && z < 2.8), "not on the deck: {core:?}");
    assert!(path.last().unwrap()[1] > 50.0, "did not get across: {:?}", path.last());
    // lower road, from the west, heading east: under the deck
    let mut w = world();
    let i = craft(&mut w, 44.5, 39.5, None, 16384.0, true);
    let path = drive(&mut w, i, 200);
    let core: Vec<f32> = path.iter().filter(|p| in_core(**p)).map(|p| p[2]).collect();
    assert!(core.len() > 3, "the craft never crossed: {:?}", path.last());
    assert!(core.iter().all(|&z| z > 0.2 && z < 0.8), "not under the deck: {core:?}");
    assert!(path.last().unwrap()[0] > 66.0, "did not get through: {:?}", path.last());
}

/// Under the deck a craft is held below the underside even when pushed up (it can't be lifted into
/// the blocks and onto the deck); on the deck nothing holds it down.
#[test]
fn underside_holds_crafts_down() {
    let mut w = world();
    let i = craft(&mut w, 59.5, 39.5, Some(0.0), 16384.0, true);
    for s in &mut w.crafts[i].sensors {
        s.1 = 0.4; // a strong push upwards
    }
    let path = drive(&mut w, i, 40);
    assert!(path.iter().all(|p| p[2] <= 0.76), "{path:?}");
}

/// Wall segments with a height range stop only the crafts on their level.
#[test]
fn walls_apply_per_level() {
    let w = world();
    let mut c = Craft::default();
    // under the deck, moving north into the lower road's wall (row 37.5): stopped
    c.pos = [59.5, 38.0, 0.39];
    let mut mv = [0.0, -1.0, 0.0];
    w.walls(&mut c, &mut mv);
    assert!(c.pos[1] + mv[1] > 37.5, "lower craft went through its wall: {mv:?}");
    // on the deck, the same move: the lower road's wall is below it
    c.pos = [59.5, 38.0, 2.39];
    let mut mv = [0.0, -1.0, 0.0];
    w.walls(&mut c, &mut mv);
    assert_eq!(mv, [0.0, -1.0, 0.0]);
    // on the deck, moving east into the upper road's wall (column 62.5): stopped
    c.pos = [62.0, 39.5, 2.39];
    let mut mv = [1.0, 0.0, 0.0];
    w.walls(&mut c, &mut mv);
    assert!(c.pos[0] + mv[0] < 62.5, "upper craft went through its wall: {mv:?}");
    // under the deck, the same move: the upper road's wall is above it
    c.pos = [62.0, 39.5, 0.39];
    let mut mv = [1.0, 0.0, 0.0];
    w.walls(&mut c, &mut mv);
    assert_eq!(mv, [1.0, 0.0, 0.0]);
}

/// Route lookups at the crossing pick the craft's own branch.
#[test]
fn route_lookups_pick_the_level() {
    let w = world();
    let t = &w.track;
    let lower = |k: usize| t.wps[k].floor < 0.1 && t.wps[k].y == 39.5;
    let upper = |k: usize| t.wps[k].x == 59.5;
    // in the crossing's core, heading east under the deck / south on it
    let a = t.nearest_at(59.5, 39.0, 0.39);
    assert!(lower(a), "{:?}", t.wps[a]);
    let b = t.nearest_at(59.5, 39.0, 2.39);
    assert!(upper(b) && t.wps[b].y > 39.0, "{:?}", t.wps[b]);
    // along the route from the waypoint under the deck, a craft on the deck still finds the deck
    let under = (1..t.wps.len()).find(|&k| t.wps[k].x == 59.5 && t.wps[k].y == 39.5 && t.wps[k].floor == 0.0).unwrap();
    let k = t.along_route(under, 59.5, 39.5, 2.39);
    assert!(upper(k), "{:?}", t.wps[k]);
    let k = t.along_route(under, 59.5, 39.5, 0.39);
    assert!(lower(k), "{:?}", t.wps[k]);
}

/// Recovery puts a craft back on the surface of its own branch: without trucks at a waypoint on
/// its level, and a truck drops it at the next open waypoint of its branch.
#[test]
fn recovery_lands_on_the_right_surface() {
    for (z, want) in [(0.0f32, 0.0f32), (2.0, 2.0)] {
        let mut w = world();
        let yaw = if z < 1.0 { 16384.0 } else { 32768.0 };
        let i = craft(&mut w, 59.5, 39.0, Some(z), yaw, false);
        {
            let c = &mut w.crafts[i];
            c.ap_first = true;
            c.waypoint = w.track.nearest_at(c.pos[0], c.pos[1], c.pos[2]);
            c.action = A_CALL;
            c.recovery_wait = 99;
        }
        w.update_craft(i, HumanIn::default());
        let c = &w.crafts[i];
        let wp = w.track.wps[c.waypoint];
        assert_eq!(wp.floor, want, "recovered at {:?}", c.pos);
        assert!((c.pos[2] - (want + 0.5)).abs() < 0.01, "recovered at {:?}", c.pos);
        // it then drives on, on that surface
        for _ in 0..30 {
            w.update_craft(i, HumanIn::default());
        }
        let c = &w.crafts[i];
        let ground = w.lvl.surface_below(c.pos[0], c.pos[1], c.pos[2]);
        assert!((ground - want).abs() < 0.01 && c.pos[2] - ground < 0.6, "after recovery at {:?} over {ground}", c.pos);
        // a truck would drop it on its branch, where no deck is above
        let d = w.drop_waypoint(c);
        let p = w.track.wps[d];
        assert!(w.lvl.ceiling_above(p.x, p.y, p.floor + 0.1).is_none(), "{p:?}");
        if want == 0.0 {
            assert!(p.y == 39.5 && p.x > 59.5 && p.floor == 0.0, "{p:?}");
        } else {
            assert!(p.x == 59.5 && p.y >= 39.5, "{p:?}");
        }
    }
}

/// Computer players lap the knot: under the deck on the lower road, over it on the upper road,
/// never jumping to the other branch, and the deck's checkpoint counts only on the deck.
#[test]
fn ai_follows_its_branch() {
    let mut w = world();
    let cells = w.track.start_cells.clone();
    for k in 0..4 {
        let (x, y) = cells[k * 3];
        let h = w.track.start_heading;
        craft(&mut w, x as f32 + 0.5, y as f32 + 0.5, None, h, false);
        w.crafts[k].lap = 0;
    }
    let deck_cp = w.track.checkpoints.iter().position(|c| c.value == 1).unwrap();
    let (mut under, mut over) = (0, 0);
    let mut last_wp: Vec<usize> = vec![0; 4];
    let mut recoveries = 0;
    for _ in 0..4000 {
        w.frame += 1;
        for i in 0..4 {
            let before = (w.crafts[i].waypoint, w.crafts[i].target_cp, w.crafts[i].action);
            w.update_craft(i, HumanIn::default());
            let c = &w.crafts[i];
            // crafts out of fuel (the level has no chargers) are recovered (no trucks: put back after 100 ticks) on
            // their own level
            if c.action != before.2 {
                recoveries += (c.action == A_RESET) as u32;
                if c.action == A_RESET {
                    let p = w.track.wps[c.waypoint];
                    assert!(super::super::track::on_level(c.pos[2], p.floor, p.floor), "{} put back at {:?} for {p:?}", c.slot, c.pos);
                }
                last_wp[i] = 0;
                continue;
            }
            // the route is followed link by link
            if before.0 != 0 && c.waypoint != before.0 && last_wp[i] != 0 {
                assert_eq!(c.waypoint, w.track.wps[before.0].child, "{} jumped from {:?} to {:?} at {:?}", c.slot, w.track.wps[before.0], w.track.wps[c.waypoint], c.pos);
            }
            last_wp[i] = c.waypoint;
            // the deck's checkpoint is passed on the deck only
            if before.1 == deck_cp && c.target_cp != deck_cp {
                assert!(c.pos[2] > 1.6, "deck checkpoint taken at {:?}", c.pos);
            }
            if in_core(c.pos) {
                let m = c.last_move;
                if m[0].abs() > m[1].abs() {
                    assert!(c.pos[2] < 1.0, "on the lower road at {:?}", c.pos);
                    under += 1;
                } else {
                    assert!(c.pos[2] > 2.2, "on the upper road at {:?}", c.pos);
                    over += 1;
                }
            }
        }
    }
    assert!(under > 20 && over > 20, "under {under}, over {over}");
    println!("under {under}, over {over}, recoveries {recoveries}, laps {:?}", w.crafts.iter().map(|c| c.lap).collect::<Vec<_>>());
    for c in &w.crafts {
        assert!(c.lap >= 3, "{} did {} laps (at {:?})", c.slot, c.lap, c.pos);
    }
}

/// Pickups stand on their level, and only crafts on that level take them.
#[test]
fn pickups_on_their_level() {
    let mut w = world();
    let items: Vec<(usize, [f32; 3])> = w.things.iter().enumerate().filter(|(_, t)| matches!(t.kind, TK::Collect { .. })).map(|(k, t)| (k, t.pos)).collect();
    assert_eq!(items.len(), 2);
    let under = items.iter().find(|i| i.1[2] == 0.0).expect("an item on the road").0;
    let over = items.iter().find(|i| i.1[2] == 2.0).expect("an item on the deck").0;
    // a craft on the lower road through the crossing, at the items' row
    let i = craft(&mut w, 44.5, 39.5, None, 16384.0, true);
    for _ in 0..150 {
        w.frame += 1;
        w.update_craft(i, HumanIn { accel: true, ..Default::default() });
        w.update_things();
    }
    assert!(w.crafts[i].pos[0] > 66.0);
    assert!(w.things[under].hidden > 0, "the road's item was not taken");
    assert_eq!(w.things[over].hidden, 0, "the deck's item was taken from under the deck");
}

/// The chase camera of a craft under the deck stays below the underside; on the deck above the top.
#[test]
fn chase_camera_respects_the_deck() {
    use super::super::camera::{RaceCamera, ViewData};
    let w = world();
    for (z, yaw) in [(0.39f32, 16384.0f32), (2.39, 32768.0)] {
        for action in [8u8, 9, 10] {
            let mut cam = RaceCamera::new();
            // just past the deck's middle, so the eye behind the craft is over the crossing too
            let v = ViewData { pos: [60.5, 40.5, z], yaw, pitch: 0.0, roll: 0.0 };
            for _ in 0..20 {
                cam.run(&w, &v, action, 1.0 / 8.0, None, [false; 6]);
            }
            let e = cam.eye(&w);
            assert!(in_core([e.x, e.row, 0.0]), "eye left the crossing: {} {}", e.x, e.row);
            if z < 1.0 {
                assert!(e.height < 1.0 - 0.1 && e.height > 0.0, "action {action}: eye at {} under a deck at 1..2", e.height);
            } else {
                assert!(e.height > 2.0 + 0.1, "action {action}: eye at {} on a deck with its top at 2", e.height);
            }
            assert!(!w.lvl.solid_at(e.x, e.row, e.height));
        }
    }
}
