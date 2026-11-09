//! The window's wgpu device, for documents that render with the GPU themselves (the 3D preview).
use eframe::egui_wgpu::RenderState;
use std::sync::OnceLock;

static STATE: OnceLock<RenderState> = OnceLock::new();

pub fn set(rs: RenderState) {
    let _ = STATE.set(rs);
}

pub fn state() -> Option<&'static RenderState> {
    STATE.get()
}
