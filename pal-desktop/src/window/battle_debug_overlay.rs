//! Read-only enemy information drawn into the native 320x200 battle frame.

use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::battle::{BattleState, BattleStatus};

use crate::renderer::Renderer;

use super::draw::{draw_debug_text, fill_rect, stroke_rect};

const DETAIL_WIDTH: i32 = 148;
const MAX_OVERVIEW_WIDTH: i32 = 308;
const PANEL_MARGIN: i32 = 6;
const HEADER_HEIGHT: i32 = 19;
const OVERVIEW_ROW_HEIGHT: i32 = 27;
const DETAIL_HEIGHT: i32 = 108;

const PANEL: [u8; 4] = [31, 16, 9, 214];
const ROW: [u8; 4] = [52, 28, 16, 196];
const ROW_SELECTED: [u8; 4] = [42, 52, 48, 218];
const BORDER: [u8; 4] = [225, 191, 126, 255];
const SEPARATOR: [u8; 4] = [142, 105, 68, 255];
const TEXT: [u8; 4] = [244, 230, 190, 255];
const MUTED: [u8; 4] = [194, 157, 108, 255];
const ACCENT: [u8; 4] = [82, 218, 207, 255];
const GOOD: [u8; 4] = [83, 210, 125, 255];
const WARNING: [u8; 4] = [239, 180, 67, 255];
const DANGER: [u8; 4] = [232, 84, 71, 255];
const BAR_TRACK: [u8; 4] = [71, 45, 28, 255];

const ENEMY_LABEL: &[u8] = &[0xbc, 0xc4, 0xb1, 0xa1]; // 敵情
const ATTACK_LABEL: &[u8] = &[0xa7, 0xf0]; // 攻
const DEFENSE_LABEL: &[u8] = &[0xa8, 0xbe]; // 防
const PHYSICAL_LABEL: &[u8] = &[0xaa, 0xab]; // 物
const STATUS_LABEL: &[u8] = &[0xaa, 0xac, 0xba, 0x41]; // 狀態
const POISON_LABEL: &[u8] = &[0xac, 0x72]; // 毒
const ELEMENT_LABELS: [&[u8]; 5] = [
    &[0xad, 0xb7], // 風
    &[0xb9, 0x70], // 雷
    &[0xa4, 0xf4], // 水
    &[0xa4, 0xf5], // 火
    &[0xa4, 0x67], // 土
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct StatusView {
    label: &'static [u8],
    rounds: u16,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EnemyView {
    index: usize,
    name: Vec<u8>,
    hp: u16,
    max_hp: u16,
    alive: bool,
    level: u16,
    attack: u16,
    defense: u16,
    physical_resistance: u16,
    elemental_resistance: [u16; pal_assets::battle::MAGIC_ELEMENT_COUNT],
    statuses: Vec<StatusView>,
    poisoned: bool,
}

impl EnemyView {
    fn display_hp(&self) -> u16 {
        u16::try_from((self.hp as i16).max(0))
            .unwrap_or(0)
            .min(self.max_hp)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BattleAssistSnapshot {
    round: u32,
    selected_enemy: Option<usize>,
    enemies: Vec<EnemyView>,
}

impl BattleAssistSnapshot {
    fn capture(battle: &BattleState, text: &TextLibrary, selected_enemy: Option<usize>) -> Self {
        let enemies = battle
            .enemies
            .iter()
            .enumerate()
            .filter(|(_, enemy)| enemy.object_id != 0)
            .map(|(index, enemy)| EnemyView {
                index,
                name: text
                    .word(usize::from(enemy.object_id))
                    .unwrap_or_default()
                    .to_vec(),
                hp: enemy.hp,
                max_hp: enemy.max_hp,
                alive: enemy.is_alive(),
                level: enemy.level,
                attack: enemy.effective_attack_strength(),
                defense: enemy.effective_defense(),
                physical_resistance: enemy.physical_resistance,
                elemental_resistance: enemy.elemental_resistance,
                statuses: BattleStatus::ALL
                    .into_iter()
                    .filter_map(|status| {
                        let rounds = enemy.statuses.duration(status);
                        (rounds != 0).then_some(StatusView {
                            label: status_label(status),
                            rounds,
                        })
                    })
                    .collect(),
                poisoned: enemy.poisons.iter().any(|poison| poison.object_id != 0),
            })
            .collect::<Vec<_>>();
        Self {
            round: battle.round(),
            selected_enemy: selected_enemy
                .filter(|index| enemies.iter().any(|enemy| enemy.index == *index)),
            enemies,
        }
    }

    fn selected_enemy(&self) -> Option<&EnemyView> {
        let selected = self.selected_enemy?;
        self.enemies.iter().find(|enemy| enemy.index == selected)
    }
}

fn status_label(status: BattleStatus) -> &'static [u8] {
    match status {
        BattleStatus::Confused => &[0xb6, 0xc3],   // 亂
        BattleStatus::Paralyzed => &[0xa9, 0x77],  // 定
        BattleStatus::Sleep => &[0xaf, 0x76],      // 眠
        BattleStatus::Silence => &[0xab, 0xca],    // 封
        BattleStatus::Puppet => &[0xb3, 0xc8],     // 傀
        BattleStatus::Bravery => &[0xab, 0x69],    // 勇
        BattleStatus::Protect => &[0xc5, 0x40],    // 護
        BattleStatus::Haste => &[0xb3, 0x74],      // 速
        BattleStatus::DualAttack => &[0xb3, 0x73], // 連
    }
}

pub(super) fn render_battle_assist(
    renderer: &mut Renderer,
    battle: &BattleState,
    text: &TextLibrary,
    font: &BitmapFont,
    selected_enemy: Option<usize>,
) {
    let snapshot = BattleAssistSnapshot::capture(battle, text, selected_enemy);
    let panel_width =
        battle_assist_width(snapshot.enemies.len(), snapshot.selected_enemy().is_some());
    let panel_height =
        battle_assist_height(snapshot.enemies.len(), snapshot.selected_enemy().is_some());
    let panel_x = i32::try_from(renderer.width).unwrap_or(i32::MAX) - panel_width - PANEL_MARGIN;
    let panel_y = PANEL_MARGIN;

    blend_rect(renderer, panel_x, panel_y, panel_width, panel_height, PANEL);
    stroke_rect(
        renderer,
        panel_x,
        panel_y,
        panel_width,
        panel_height,
        BORDER,
    );
    renderer.draw_big5_text_shadowed(font, ENEMY_LABEL, panel_x + 7, panel_y + 2, 0x4f);
    draw_debug_text(
        renderer,
        panel_x + 43,
        panel_y + 6,
        &format!(
            "x{}",
            snapshot.enemies.iter().filter(|enemy| enemy.alive).count()
        ),
        MUTED,
    );
    draw_debug_text_right(
        renderer,
        panel_x + panel_width - 7,
        panel_y + 6,
        &format!("R{:02}", snapshot.round),
        ACCENT,
    );
    fill_rect(
        renderer,
        panel_x + 1,
        panel_y + HEADER_HEIGHT - 1,
        panel_width - 2,
        1,
        SEPARATOR,
    );

    if let Some(enemy) = snapshot.selected_enemy() {
        draw_enemy_details(renderer, font, enemy, panel_x, panel_y + HEADER_HEIGHT);
    } else {
        draw_enemy_overview(
            renderer,
            font,
            &snapshot.enemies,
            panel_x,
            panel_y + HEADER_HEIGHT,
            panel_width,
        );
    }
}

fn battle_assist_width(enemy_count: usize, target_details: bool) -> i32 {
    if target_details {
        DETAIL_WIDTH
    } else {
        (i32::try_from(enemy_count).unwrap_or(1).max(1) * 112)
            .clamp(DETAIL_WIDTH, MAX_OVERVIEW_WIDTH)
    }
}

fn battle_assist_height(_enemy_count: usize, target_details: bool) -> i32 {
    if target_details {
        DETAIL_HEIGHT
    } else {
        HEADER_HEIGHT + OVERVIEW_ROW_HEIGHT + 2
    }
}

fn draw_enemy_overview(
    renderer: &mut Renderer,
    font: &BitmapFont,
    enemies: &[EnemyView],
    panel_x: i32,
    start_y: i32,
    panel_width: i32,
) {
    let count = i32::try_from(enemies.len()).unwrap_or(1).max(1);
    let content_x = panel_x + 2;
    let content_width = panel_width - 4;
    for (column, enemy) in enemies.iter().enumerate() {
        let column = i32::try_from(column).unwrap_or(0);
        let left = content_x + content_width * column / count;
        let right = content_x + content_width * (column + 1) / count;
        let width = right - left;
        let y = start_y;
        blend_rect(
            renderer,
            left,
            y + 1,
            (width - 1).max(1),
            OVERVIEW_ROW_HEIGHT - 2,
            ROW,
        );
        fill_rect(
            renderer,
            left + 1,
            y + 2,
            2,
            OVERVIEW_ROW_HEIGHT - 4,
            hp_color(enemy),
        );
        let max_name_glyphs = match width {
            0..=68 => 2,
            69..=92 => 3,
            _ => 4,
        };
        draw_enemy_name(
            renderer,
            font,
            enemy,
            left + 5,
            y + 2,
            max_name_glyphs,
            0x4f,
        );
        let level = if enemy.alive {
            format!("LV{}", enemy.level)
        } else {
            "KO".to_owned()
        };
        if width >= 104 {
            draw_debug_text_right(
                renderer,
                right - 5,
                y + 6,
                &level,
                if enemy.alive { MUTED } else { DANGER },
            );
        }
        let hp = if width >= 96 {
            format!("HP {}/{}", enemy.display_hp(), enemy.max_hp)
        } else {
            format!("HP {}", enemy.display_hp())
        };
        draw_debug_text(
            renderer,
            left + 5,
            y + 18,
            &hp,
            if enemy.alive { TEXT } else { DANGER },
        );
        draw_hp_bar(renderer, enemy, left + 4, y + 25, width - 9, 2);
    }
}

fn draw_enemy_details(
    renderer: &mut Renderer,
    font: &BitmapFont,
    enemy: &EnemyView,
    panel_x: i32,
    content_y: i32,
) {
    blend_rect(
        renderer,
        panel_x + 2,
        content_y + 1,
        DETAIL_WIDTH - 4,
        DETAIL_HEIGHT - HEADER_HEIGHT - 3,
        ROW_SELECTED,
    );
    fill_rect(
        renderer,
        panel_x + 3,
        content_y + 2,
        2,
        DETAIL_HEIGHT - HEADER_HEIGHT - 5,
        ACCENT,
    );
    draw_enemy_name(renderer, font, enemy, panel_x + 8, content_y + 2, 6, 0x8d);
    draw_debug_text_right(
        renderer,
        panel_x + DETAIL_WIDTH - 8,
        content_y + 6,
        &format!("LV{}", enemy.level),
        ACCENT,
    );
    draw_debug_text(
        renderer,
        panel_x + 8,
        content_y + 18,
        &format!("HP {}/{}", enemy.display_hp(), enemy.max_hp),
        TEXT,
    );
    draw_hp_bar(
        renderer,
        enemy,
        panel_x + 8,
        content_y + 26,
        DETAIL_WIDTH - 16,
        4,
    );

    renderer.draw_big5_text_shadowed(font, ATTACK_LABEL, panel_x + 8, content_y + 31, 0x4f);
    draw_debug_text(
        renderer,
        panel_x + 25,
        content_y + 36,
        &enemy.attack.to_string(),
        TEXT,
    );
    renderer.draw_big5_text_shadowed(font, DEFENSE_LABEL, panel_x + 57, content_y + 31, 0x4f);
    draw_debug_text(
        renderer,
        panel_x + 74,
        content_y + 36,
        &enemy.defense.to_string(),
        TEXT,
    );
    renderer.draw_big5_text_shadowed(font, PHYSICAL_LABEL, panel_x + 103, content_y + 31, 0x4f);
    draw_debug_text(
        renderer,
        panel_x + 120,
        content_y + 36,
        &enemy.physical_resistance.to_string(),
        TEXT,
    );

    for (index, (&label, resistance)) in ELEMENT_LABELS
        .iter()
        .zip(enemy.elemental_resistance)
        .enumerate()
    {
        let x = panel_x + 8 + i32::try_from(index).unwrap_or(0) * 27;
        renderer.draw_big5_text_shadowed(font, label, x, content_y + 48, 0x4f);
        draw_debug_text(
            renderer,
            x + 17,
            content_y + 53,
            &resistance.to_string(),
            MUTED,
        );
    }

    renderer.draw_big5_text_shadowed(font, STATUS_LABEL, panel_x + 8, content_y + 65, 0x4f);
    draw_enemy_statuses(renderer, font, enemy, panel_x + 43, content_y + 65);
}

fn draw_enemy_statuses(
    renderer: &mut Renderer,
    font: &BitmapFont,
    enemy: &EnemyView,
    x: i32,
    y: i32,
) {
    let right = x + 96;
    let mut cursor = x;
    if enemy.poisoned {
        renderer.draw_big5_text_shadowed(font, POISON_LABEL, cursor, y, 0x2d);
        cursor += 20;
    }
    for status in &enemy.statuses {
        if cursor + 22 > right {
            break;
        }
        renderer.draw_big5_text_shadowed(font, status.label, cursor, y, 0x2d);
        draw_debug_text(
            renderer,
            cursor + 16,
            y + 5,
            &status.rounds.to_string(),
            WARNING,
        );
        cursor += 24;
    }
    if cursor == x {
        draw_debug_text(renderer, x, y + 5, "--", MUTED);
    }
}

fn draw_enemy_name(
    renderer: &mut Renderer,
    font: &BitmapFont,
    enemy: &EnemyView,
    x: i32,
    y: i32,
    max_glyphs: usize,
    color: u8,
) {
    let bytes = enemy.name.len().min(max_glyphs.saturating_mul(2)) & !1;
    if bytes == 0 {
        draw_debug_text(
            renderer,
            x,
            y + 5,
            &format!("ENEMY {}", enemy.index + 1),
            TEXT,
        );
    } else {
        renderer.draw_big5_text_shadowed(font, &enemy.name[..bytes], x, y, color);
    }
}

fn draw_debug_text_right(
    renderer: &mut Renderer,
    right_x: i32,
    y: i32,
    text: &str,
    color: [u8; 4],
) {
    let width = i32::try_from(text.chars().count()).unwrap_or(0) * 6 - 1;
    draw_debug_text(renderer, right_x - width, y, text, color);
}

fn draw_hp_bar(
    renderer: &mut Renderer,
    enemy: &EnemyView,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
) {
    fill_rect(renderer, x, y, width, height, BAR_TRACK);
    let filled = if enemy.max_hp == 0 {
        0
    } else {
        u32::from(enemy.display_hp()) * u32::try_from(width).unwrap_or(0) / u32::from(enemy.max_hp)
    };
    if filled != 0 {
        fill_rect(
            renderer,
            x,
            y,
            i32::try_from(filled).unwrap_or(width),
            height,
            hp_color(enemy),
        );
    }
}

fn hp_color(enemy: &EnemyView) -> [u8; 4] {
    if !enemy.alive {
        DANGER
    } else if enemy.max_hp != 0 && u32::from(enemy.display_hp()) * 4 <= u32::from(enemy.max_hp) {
        WARNING
    } else {
        GOOD
    }
}

fn blend_rect(renderer: &mut Renderer, x: i32, y: i32, width: i32, height: i32, color: [u8; 4]) {
    let surface_width = i32::try_from(renderer.width).unwrap_or(i32::MAX);
    let surface_height = i32::try_from(renderer.height).unwrap_or(i32::MAX);
    let left = x.max(0).min(surface_width);
    let top = y.max(0).min(surface_height);
    let right = x.saturating_add(width).max(0).min(surface_width);
    let bottom = y.saturating_add(height).max(0).min(surface_height);
    if left >= right || top >= bottom || color[3] == 0 {
        return;
    }
    let stride = renderer.width;
    let alpha = u32::from(color[3]);
    let screen = renderer.screen_mut();
    for row in top..bottom {
        for column in left..right {
            let index = (usize::try_from(row).unwrap_or(0) * stride
                + usize::try_from(column).unwrap_or(0))
                * 4;
            for channel in 0..3 {
                screen[index + channel] = ((u32::from(color[channel]) * alpha
                    + u32::from(screen[index + channel]) * (255 - alpha))
                    / 255) as u8;
            }
            screen[index + 3] = 255;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enemy() -> EnemyView {
        EnemyView {
            index: 0,
            name: Vec::new(),
            hp: 80,
            max_hp: 120,
            alive: true,
            level: 12,
            attack: 82,
            defense: 64,
            physical_resistance: 2,
            elemental_resistance: [1, 2, 3, 4, 5],
            statuses: Vec::new(),
            poisoned: false,
        }
    }

    #[test]
    fn status_labels_use_original_single_glyph_big5_text() {
        assert_eq!(status_label(BattleStatus::Sleep), &[0xaf, 0x76]);
        assert_eq!(status_label(BattleStatus::Haste), &[0xb3, 0x74]);
        assert_eq!(POISON_LABEL, &[0xac, 0x72]);
    }

    #[test]
    fn overview_and_details_stay_above_the_classic_battle_hud() {
        assert!(PANEL_MARGIN + battle_assist_height(5, false) < 165);
        assert!(PANEL_MARGIN + battle_assist_height(5, true) < 165);
        assert!(battle_assist_width(5, false) + PANEL_MARGIN <= 320);
        assert_eq!(battle_assist_width(1, false), DETAIL_WIDTH);
    }

    #[test]
    fn hp_display_uses_signed_word_semantics_and_static_maximum() {
        let mut enemy = enemy();
        enemy.hp = u16::MAX;
        assert_eq!(enemy.display_hp(), 0);
        enemy.hp = 200;
        assert_eq!(enemy.display_hp(), 120);
    }
}
