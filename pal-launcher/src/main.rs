use std::path::PathBuf;
use pal_assets::mkf::MkfArchive;
use pal_assets::palette::Palette;
use pal_assets::bitmap::Bitmap;

fn main() {
    let data_dir = { let mut d = PathBuf::from(env!("CARGO_MANIFEST_DIR")); d.pop(); d.push("data"); d };
    let palette = Palette::from_bytes(&MkfArchive::new(&std::fs::read(data_dir.join("DATA.MKF")).unwrap()).unwrap().read_chunk(1).unwrap()).unwrap();

    // 导出 BALL.MKF 前 3 个图标
    if let Some(arc) = MkfArchive::new(&std::fs::read(data_dir.join("BALL.MKF")).unwrap()) {
        for i in 0..3.min(arc.chunk_count()) {
            if let Some(bmp) = Bitmap::from_rle(arc.read_chunk(i).unwrap(), &palette) {
                pal_desktop::save_png(&bmp, &format!("{}/item_{}.png", data_dir.display(), i)).ok();
                println!("item_{}.png: {}x{}", i, bmp.width, bmp.height);
            }
        }
    }
    println!("Phase 2 complete!");
}
