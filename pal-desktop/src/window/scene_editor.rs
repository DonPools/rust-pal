//! Standalone, read-only scene and script inspector.

mod app;
mod content;
mod egui_app;
mod hit_test;
mod navigation;

use pal_core::map::{MAP_PIXEL_HEIGHT, MAP_PIXEL_WIDTH};

use self::egui_app::EguiSceneEditorApp;
use super::{LoadedScene, SceneEditorResources};
use crate::renderer::Renderer;

pub const SCENE_EDITOR_WIDTH: u32 = 800;
pub const SCENE_EDITOR_HEIGHT: u32 = 450;

pub(super) const CANVAS_WIDTH: u32 = SCENE_EDITOR_WIDTH;
pub(super) const CANVAS_HEIGHT: u32 = SCENE_EDITOR_HEIGHT;
pub(super) const MAX_ZOOM: u8 = 3;

/// Run the scene inspector in an eframe host independent from normal gameplay.
pub fn run_scene_editor_window<L>(
    renderer: Renderer,
    initial_scene: LoadedScene,
    resources: SceneEditorResources,
    load_scene: L,
) where
    L: FnMut(u16, &pal_core::role::RoleSprites) -> Option<LoadedScene> + 'static,
{
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_app_id("rust-pal-scene-inspector")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([960.0, 600.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Rust-PAL Scene Inspector",
        native_options,
        Box::new(move |context| {
            Ok(Box::new(EguiSceneEditorApp::new(
                context,
                renderer,
                initial_scene,
                resources,
                load_scene,
            )))
        }),
    )
    .expect("scene editor event loop failed");
}

pub(super) fn canvas_view_size(canvas: u32, zoom: u8) -> u32 {
    canvas.div_ceil(u32::from(zoom.max(1)))
}

pub(super) fn clamped_viewport(x: i32, y: i32, width: u32, height: u32) -> (i32, i32) {
    let max_x = (MAP_PIXEL_WIDTH - width as i32).max(0);
    let max_y = (MAP_PIXEL_HEIGHT - height as i32).max(0);
    (x.clamp(0, max_x), y.clamp(0, max_y))
}

pub(super) fn scale_canvas(renderer: &mut Renderer, zoom: u8) {
    if zoom <= 1 {
        return;
    }
    let source = renderer.screen().to_vec();
    let source_stride = renderer.width * 4;
    let output_stride = renderer.width * 4;
    let width = renderer.width;
    let height = renderer.height;
    let output = renderer.screen_mut();
    for y in 0..height {
        let source_y = y / usize::from(zoom);
        for x in 0..width {
            let source_x = x / usize::from(zoom);
            let source_index = source_y * source_stride + source_x * 4;
            let output_index = y * output_stride + x * 4;
            output[output_index..output_index + 4]
                .copy_from_slice(&source[source_index..source_index + 4]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_and_key_helpers_clamp_to_valid_ranges() {
        assert_eq!(clamped_viewport(-10, -20, 480, 450), (0, 0));
        assert_eq!(
            clamped_viewport(i32::MAX, i32::MAX, 480, 450),
            (MAP_PIXEL_WIDTH - 480, MAP_PIXEL_HEIGHT - 450)
        );
        assert_eq!(canvas_view_size(480, 3), 160);
    }
}
