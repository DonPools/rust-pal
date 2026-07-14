//! pal-launcher - 仙剑奇侠传 Rust 版启动入口
//!
//! 第一阶段：用 MKF 解析器读取游戏数据文件，验证解析正确性。

use std::path::PathBuf;

fn find_data_dir() -> PathBuf {
    // 从 manifest 目录向上找 data/
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.pop(); // 从 pal-launcher 到 workspace 根
    dir.push("data");
    if dir.exists() {
        return dir;
    }
    // 回退到当前目录
    PathBuf::from("data")
}

fn main() {
    let data_dir = find_data_dir();
    println!("📂 游戏数据目录: {}", data_dir.display());
    println!();

    // 列出要解析的 MKF 文件
    let mkf_files = [
        "ABC.MKF",
        "DATA.MKF",
        "MAP.MKF",
        "F.MKF",
        "FBP.MKF",
        "FIRE.MKF",
        "GOP.MKF",
        "MIDI.MKF",
        "MUS.MKF",
        "RGM.MKF",
        "RNG.MKF",
        "SSS.MKF",
        "VOC.MKF",
        "PAT.MKF",
        "BALL.MKF",
    ];

    for filename in &mkf_files {
        let path = data_dir.join(filename);
        if !path.exists() {
            println!("⚠️    {} - 文件不存在", filename);
            continue;
        }

        let data = match std::fs::read(&path) {
            Ok(d) => d,
            Err(e) => {
                println!("❌    {} - 读取失败: {}", filename, e);
                continue;
            }
        };

        match pal_assets::mkf::MkfArchive::new(&data) {
            Some(archive) => {
                let count = archive.chunk_count();
                let total_size = archive
                    .chunk_sizes()
                    .iter()
                    .sum::<usize>();
                let first_size = archive
                    .read_chunk(0)
                    .map(|c| c.len())
                    .unwrap_or(0);
                println!(
                    "✅    {:<12}  {} chunks,  {} bytes (首 chunk: {} bytes)",
                    filename,
                    count,
                    total_size,
                    first_size,
                );
            }
            None => {
                println!("❌    {} - 解析失败", filename);
            }
        }
    }
}
