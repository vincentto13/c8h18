//! The wgpu device for documents that render with the GPU themselves (the 3D preview): the
//! window's, or without a window (MCP headless mode) one of our own.
use eframe::egui_wgpu::RenderState;
use std::sync::OnceLock;

static STATE: OnceLock<RenderState> = OnceLock::new();

pub fn set(rs: RenderState) {
    let _ = STATE.set(rs);
}

pub fn state() -> Option<&'static RenderState> {
    STATE.get()
}

type DeviceQueue = (eframe::wgpu::Device, eframe::wgpu::Queue);
static HEADLESS: OnceLock<Result<DeviceQueue, String>> = OnceLock::new();

/// The window's device, else a headless one (created on first use).
pub fn device() -> Result<DeviceQueue, String> {
    if let Some(rs) = state() {
        return Ok((rs.device.clone(), rs.queue.clone()));
    }
    HEADLESS
        .get_or_init(|| {
            use eframe::wgpu;
            pollster::block_on(async {
                let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
                let adapter = instance.request_adapter(&wgpu::RequestAdapterOptions::default()).await.map_err(|e| format!("no GPU adapter: {e}"))?;
                adapter.request_device(&wgpu::DeviceDescriptor { label: Some("c8h18-edit headless"), ..Default::default() }).await.map_err(|e| format!("no GPU device: {e}"))
            })
        })
        .clone()
}
