//! Level geometry shared by the game's renderer and the editor's 3D preview: the vertex format
//! and the level mesh (floor quads and block faces, spec 08 §5.2).
use bytemuck::{Pod, Zeroable};

pub mod terrain;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable, Debug, Default)]
pub struct Vertex {
    /// world: x (east, cells), height (cells), row (south, cells)
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    /// light-table row (32 = unchanged)
    pub light: f32,
    /// low byte: 0 terrain texture, 1 model atlas, 2 flat colour, 3 billboard; >>8: fog 0 none, 1 terrain, 2 sprite
    pub kind: u32,
    /// terrain: texture id; flat: palette index; billboard: blend byte
    pub param: u32,
}

pub const K_TERRAIN: u32 = 0 | (1 << 8);
pub const K_ATLAS: u32 = 1;
pub const K_FLAT: u32 = 2;
pub const K_SPRITE: u32 = 3 | (2 << 8);
pub const K_FLAT_TERRAIN: u32 = 2 | (1 << 8);
