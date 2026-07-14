//! pal-desktop - 仙剑奇侠传 Rust 版桌面入口
//! 负责窗口创建、事件循环、渲染

pub mod renderer;
pub mod window;

use pal_assets::bitmap::Bitmap;

/// 将位图保存为 PNG 文件
pub fn save_png(bitmap: &Bitmap, path: &str) -> Result<(), String> {
    use image::RgbaImage;
    let img = RgbaImage::from_raw(
        bitmap.width as u32,
        bitmap.height as u32,
        bitmap.rgba.clone(),
    )
    .ok_or("Failed to create image buffer")?;

    img.save(path)
        .map_err(|e| format!("Failed to save PNG: {}", e))?;
    Ok(())
}
