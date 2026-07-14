//! rust-pal - 仙剑奇侠传 Rust 版
//! 游戏主入口，启动整个游戏

pub fn start() {
    pal_core::init();
    pal_assets::load_assets();
    pal_desktop::run();
}
