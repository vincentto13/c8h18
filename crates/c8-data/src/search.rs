//! Search-ring grid, `DATA/SRCH0-0.DAT` (spec 01 §10): a 32 × 32 byte grid describing 16 rings of
//! cells around a centre, which the big explosions use to place their effects ring by ring.
//!
//! Byte values: `k` (0..15) marks the start cell of ring k, `0x10 + k` another cell of ring k,
//! `0x20` a cell outside the rings and `0xFE` the unused border (last row and column).

pub const SIZE: usize = 32;
pub const OUTSIDE: u8 = 0x20;
pub const BORDER: u8 = 0xFE;
pub const RINGS: usize = 16;

/// Walk directions (dx, dy), clockwise from east.
const DIRS: [(i32, i32); 8] = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)];

/// The ring a grid byte belongs to, if any.
pub fn ring_of(b: u8) -> Option<usize> {
    (b < 0x20).then_some((b & 15) as usize)
}

/// The rings built from a grid as the game builds them: each ring is the list of cells in walk
/// order, as (cell index, offset from the centre). Rings without a start cell stop the build.
pub struct Rings {
    pub centre: (i32, i32),
    pub rings: Vec<Vec<(usize, (i32, i32))>>,
}

impl Rings {
    pub fn build(grid: &[u8]) -> Rings {
        let mut g: Vec<i16> = grid.iter().map(|&b| b as i16).collect();
        g.resize(SIZE * SIZE, BORDER as i16);
        let mut rings = Vec::new();
        let mut centre = (15i32, 15i32);
        for k in 0..RINGS as i16 {
            let Some(start) = g.iter().position(|&b| b == k) else { break };
            let (mut cx, mut cy) = ((start % SIZE) as i32, (start / SIZE) as i32);
            if k == 0 {
                centre = (cx, cy);
            }
            let mut ring = vec![(start, (cx - centre.0, cy - centre.1))];
            g[start] = -1;
            let mut base = 0usize;
            'walk: loop {
                for d in [2usize, 0, 1, 3, 4, 5, 6, 7] {
                    let (dx, dy) = DIRS[(d + base) & 7];
                    let (nx, ny) = (cx + dx, cy + dy);
                    if nx < 0 || ny < 0 || nx >= SIZE as i32 || ny >= SIZE as i32 {
                        continue;
                    }
                    let i = ny as usize * SIZE + nx as usize;
                    if g[i] >= 0 && (g[i] & 15) == k && g[i] < 0x20 {
                        base = (base + d) & 6;
                        ring.push((i, (nx - centre.0, ny - centre.1)));
                        g[i] = -1;
                        (cx, cy) = (nx, ny);
                        continue 'walk;
                    }
                }
                break;
            }
            rings.push(ring);
        }
        Rings { centre, rings }
    }

    /// Ring cells of the grid that the walk never reaches (the game ignores them).
    pub fn unreached(&self, grid: &[u8]) -> Vec<usize> {
        let mut seen = vec![false; SIZE * SIZE];
        for r in &self.rings {
            for &(i, _) in r {
                seen[i] = true;
            }
        }
        (0..SIZE * SIZE).filter(|&i| ring_of(grid[i]).is_some() && !seen[i]).collect()
    }

    /// The offsets only, as the game uses them.
    pub fn offsets(&self) -> Vec<Vec<(i32, i32)>> {
        self.rings.iter().map(|r| r.iter().map(|&(_, o)| o).collect()).collect()
    }
}

/// Problems with a grid: rings without a start cell, start cells that are not in the ring's
/// first row-major position, unreached cells.
pub fn check(grid: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    if grid.len() != SIZE * SIZE {
        out.push(format!("{} bytes, the grid has {}", grid.len(), SIZE * SIZE));
        return out;
    }
    for k in 0..RINGS {
        let starts = grid.iter().filter(|&&b| b as usize == k).count();
        if starts == 0 {
            out.push(format!("ring {k} has no start cell{}", if k == 0 { " (and so no centre)" } else { "" }));
        } else if starts > 1 {
            out.push(format!("ring {k} has {starts} start cells; the first one (row by row) is used"));
        }
    }
    let rings = Rings::build(grid);
    let missed = rings.unreached(grid);
    if !missed.is_empty() {
        out.push(format!("{} ring cells are not reached by the walk and are ignored", missed.len()));
    }
    out
}

/// The ring of a cell at squared distance `d2` from the centre: ring k starts at d² = k² − k,
/// i.e. the distance rounded, with the exact half-way squares (2, 20, 72, ...) going outwards,
/// which keeps every ring a thin closed chain.
pub fn ring_at(d2: i32) -> usize {
    if d2 <= 0 {
        return 0;
    }
    // largest k with k² − k ≤ d2  ⇔  (2k − 1)² ≤ 4·d2 + 1
    let s = ((4 * d2 + 1) as f64).sqrt() as i32;
    let s = (s - 1..=s + 1).filter(|v| v * v <= 4 * d2 + 1).max().unwrap();
    ((s + 1) / 2) as usize
}

/// A fresh grid: concentric rings around (15, 15) by [`ring_at`], start cells straight above the
/// centre. The original differs in 16 cells (d² = 90 and 181 sit one ring off, by hand).
pub fn generate() -> Vec<u8> {
    let c = 15i32;
    let mut g = vec![OUTSIDE; SIZE * SIZE];
    for y in 0..SIZE as i32 {
        for x in 0..SIZE as i32 {
            let i = (y * SIZE as i32 + x) as usize;
            let last = SIZE as i32 - 1;
            // the border is the last row and column, except the top right cell (as the original)
            if y == last || (x == last && y > 0) {
                g[i] = BORDER;
                continue;
            }
            let r = ring_at((x - c).pow(2) + (y - c).pow(2));
            if r < RINGS && x < last {
                g[i] = if x == c && y <= c { r as u8 } else { 0x10 + r as u8 };
            }
        }
    }
    g
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_grid_is_complete_and_close_to_the_original() {
        let g = generate();
        assert!(check(&g).is_empty(), "{:?}", check(&g));
        let r = Rings::build(&g);
        assert_eq!(r.rings.len(), RINGS);
        assert_eq!(r.centre, (15, 15));
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/DATA/SRCH0-0.DAT");
        let Ok(raw) = std::fs::read(p) else { return };
        let o = crate::rnc::maybe_unpack(raw).unwrap();
        assert!(check(&o).is_empty(), "{:?}", check(&o));
        let diff = o.iter().zip(&g).filter(|(a, b)| a != b).count();
        assert!(diff <= 16, "{diff} cells differ");
    }
}
