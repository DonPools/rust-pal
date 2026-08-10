//! Cross-platform discovery for a system font that covers Chinese text.

use std::env;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
pub(super) struct ChineseFontSource {
    pub(super) path: PathBuf,
    pub(super) name: String,
    pub(super) collection_index: u32,
}

pub(super) fn chinese_font_candidates() -> Vec<ChineseFontSource> {
    let mut candidates = Vec::new();
    if let Some(path) = env::var_os("RUST_PAL_DEBUG_FONT") {
        candidates.push(ChineseFontSource {
            path: PathBuf::from(path),
            name: "自定义调试字体".to_owned(),
            collection_index: 0,
        });
    }
    #[cfg(target_os = "macos")]
    candidates.extend([
        ChineseFontSource {
            path: PathBuf::from("/System/Library/Fonts/PingFang.ttc"),
            name: "苹方".to_owned(),
            collection_index: 0,
        },
        ChineseFontSource {
            path: PathBuf::from("/System/Library/Fonts/Hiragino Sans GB.ttc"),
            name: "冬青黑体简体".to_owned(),
            collection_index: 0,
        },
        ChineseFontSource {
            path: PathBuf::from("/System/Library/Fonts/STHeiti Medium.ttc"),
            name: "华文黑体".to_owned(),
            collection_index: 0,
        },
    ]);
    #[cfg(target_os = "windows")]
    if let Some(windows) = env::var_os("WINDIR") {
        let fonts = PathBuf::from(windows).join("Fonts");
        candidates.extend([
            ChineseFontSource {
                path: fonts.join("msyh.ttc"),
                name: "微软雅黑".to_owned(),
                collection_index: 0,
            },
            ChineseFontSource {
                path: fonts.join("msyhbd.ttc"),
                name: "微软雅黑粗体".to_owned(),
                collection_index: 0,
            },
            ChineseFontSource {
                path: fonts.join("simhei.ttf"),
                name: "黑体".to_owned(),
                collection_index: 0,
            },
        ]);
    }
    #[cfg(target_os = "linux")]
    candidates.extend([
        ChineseFontSource {
            path: PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
            name: "Noto Sans CJK SC".to_owned(),
            collection_index: 2,
        },
        ChineseFontSource {
            path: PathBuf::from("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc"),
            name: "Noto Sans CJK SC".to_owned(),
            collection_index: 2,
        },
        ChineseFontSource {
            path: PathBuf::from("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc"),
            name: "文泉驿微米黑".to_owned(),
            collection_index: 0,
        },
    ]);
    if let Some((path, collection_index)) = fontconfig_match() {
        candidates.push(ChineseFontSource {
            path,
            name: "系统中文字体".to_owned(),
            collection_index,
        });
    }
    candidates
}

fn fontconfig_match() -> Option<(PathBuf, u32)> {
    let output = Command::new("fc-match")
        .args(["-f", "%{file}\t%{index}", "Noto Sans CJK SC:lang=zh-cn"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let output = String::from_utf8_lossy(&output.stdout);
    let (path, index) = output.trim().rsplit_once('\t')?;
    Some((PathBuf::from(path), index.parse().ok()?))
}
