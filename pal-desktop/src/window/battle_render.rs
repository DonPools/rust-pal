use pal_assets::battle::BattleSpriteArchive;
use pal_assets::bitmap::Bitmap;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::battle::{BattleEvent, BattlePhase, BattleResult, BattleState};

use super::battle_update::ACTION_EVENT_TICKS;
use super::draw::{draw_debug_text, draw_number, fill_rect, stroke_rect};
use crate::renderer::Renderer;

pub struct BattleRenderResources<'a> {
    pub enemy_sprites: &'a BattleSpriteArchive,
    pub player_sprites: &'a BattleSpriteArchive,
    pub backgrounds: &'a [Option<Bitmap>],
    pub text: &'a TextLibrary,
    pub font: &'a BitmapFont,
}

#[derive(Clone, Copy)]
pub struct BattleRenderState {
    pub selected_enemy: usize,
    pub selected_command: usize,
    pub ticks: u64,
    pub event: Option<BattleEvent>,
    pub event_ticks: u16,
}

pub fn render_battle(
    renderer: &mut Renderer,
    battle: &BattleState,
    resources: BattleRenderResources<'_>,
    state: BattleRenderState,
) {
    let BattleRenderState {
        selected_enemy,
        selected_command,
        ticks,
        event,
        event_ticks,
    } = state;
    if let Some(background) = resources
        .backgrounds
        .get(usize::from(battle.battlefield))
        .and_then(Option::as_ref)
    {
        renderer.blit_bitmap(background, 0, 0);
    } else {
        renderer.clear_black();
    }

    for (index, enemy) in battle.enemies.iter().enumerate() {
        let is_defeat_event = matches!(
            event,
            Some(
                BattleEvent::PlayerAttack {
                    enemy: target,
                    defeated: true,
                    ..
                } | BattleEvent::PlayerMagic {
                    enemy: target,
                    defeated: true,
                    ..
                } | BattleEvent::SimulatedMagic {
                    enemy: target,
                    defeated: true,
                    ..
                } | BattleEvent::EnemyConfusedAttack {
                    target,
                    defeated: true,
                    ..
                }
            ) if target == index
        );
        if !enemy.is_alive() && !is_defeat_event {
            continue;
        }
        let idle_frames = usize::from(enemy.idle_frames.max(1));
        let speed = u64::from(enemy.idle_animation_speed.max(1));
        let frame = match event {
            Some(BattleEvent::EnemyMagic { enemy: caster, .. })
                if caster == index && enemy.magic_frames > 0 =>
            {
                let elapsed =
                    ACTION_EVENT_TICKS.saturating_sub(event_ticks.min(ACTION_EVENT_TICKS));
                idle_frames
                    + usize::from(elapsed).min(usize::from(enemy.magic_frames).saturating_sub(1))
            }
            _ => usize::try_from(ticks / speed).unwrap_or(0) % idle_frames,
        };
        let Some(bitmap) = resources
            .enemy_sprites
            .decode_frame(usize::from(enemy.enemy_id), frame)
        else {
            continue;
        };
        let enemy_offset = match event {
            Some(BattleEvent::EnemyAttack { enemy, .. }) if enemy == index => {
                action_offset(event_ticks)
            }
            Some(BattleEvent::EnemyMagic { enemy, .. }) if enemy == index => {
                action_offset(event_ticks) / 2
            }
            Some(BattleEvent::EnemyConfusedAttack { enemy, .. }) if enemy == index => {
                action_offset(event_ticks)
            }
            _ => 0,
        };
        let x = i32::from(enemy.position.x) - i32::from(bitmap.width) / 2 + enemy_offset;
        let y = i32::from(enemy.position.y) + i32::from(enemy.y_offset) - i32::from(bitmap.height)
            + enemy_offset / 2;
        renderer.blit_rle(&bitmap, x, y);
        let is_hit = matches!(
            event,
            Some(
                BattleEvent::PlayerAttack { enemy, .. }
                    | BattleEvent::PlayerMagic { enemy, .. }
                    | BattleEvent::SimulatedMagic { enemy, .. }
                    | BattleEvent::EnemyConfusedAttack { target: enemy, .. }
            ) if enemy == index
        );
        if is_hit && event_ticks.is_multiple_of(2) {
            stroke_rect(
                renderer,
                x - 2,
                y - 2,
                i32::from(bitmap.width) + 4,
                i32::from(bitmap.height) + 4,
                [255, 80, 72, 255],
            );
        }
        if event.is_none()
            && index == selected_enemy
            && battle.phase() == BattlePhase::AwaitingCommand
        {
            draw_selector(
                renderer,
                i32::from(enemy.position.x),
                y - 5,
                [255, 248, 80, 255],
            );
        }
    }

    for (index, player) in battle.players.iter().enumerate() {
        let (mut x, mut y) = player_position(battle.players.len(), index);
        match event {
            Some(BattleEvent::PlayerAttack { player, .. }) if player == index => {
                let offset = action_offset(event_ticks);
                x -= offset;
                y -= offset / 2;
            }
            Some(BattleEvent::PlayerConfusedAttack { player, .. }) if player == index => {
                let offset = action_offset(event_ticks);
                x -= offset;
                y -= offset / 2;
            }
            Some(BattleEvent::PlayerMagic { player, .. }) if player == index => {
                y -= action_offset(event_ticks) / 3;
            }
            Some(
                BattleEvent::PlayerUseItem { player, .. }
                | BattleEvent::PlayerThrowItem { player, .. },
            ) if player == index => {
                y -= action_offset(event_ticks) / 3;
            }
            _ => {}
        }
        let sprite = usize::from(player.battle_sprite_num);
        let available = resources.player_sprites.frame_count(sprite).unwrap_or(0);
        let frame = if player.is_alive() {
            0
        } else {
            2.min(available.saturating_sub(1))
        };
        if let Some(bitmap) = resources.player_sprites.decode_frame(sprite, frame) {
            let left = x - i32::from(bitmap.width) / 2;
            let top = y - i32::from(bitmap.height);
            renderer.blit_rle(&bitmap, left, top);
            let is_hit = matches!(
                event,
                Some(
                    BattleEvent::EnemyAttack { player, .. }
                        | BattleEvent::EnemyMagic { player, .. }
                        | BattleEvent::PlayerConfusedAttack { target: player, .. }
                ) if player == index
            );
            if is_hit && event_ticks.is_multiple_of(2) {
                stroke_rect(
                    renderer,
                    left - 2,
                    top - 2,
                    i32::from(bitmap.width) + 4,
                    i32::from(bitmap.height) + 4,
                    [255, 80, 72, 255],
                );
            }
        }
    }

    if let Some(event) = event {
        render_battle_event(renderer, battle, event);
    }

    render_status(
        renderer,
        battle,
        resources.text,
        resources.font,
        selected_command,
    );
    if event.is_none() {
        if let BattlePhase::Finished(result) = battle.phase() {
            render_settlement(renderer, battle, result);
        }
    }
}

fn action_offset(ticks_remaining: u16) -> i32 {
    let elapsed = ACTION_EVENT_TICKS.saturating_sub(ticks_remaining.min(ACTION_EVENT_TICKS));
    let distance = elapsed.min(ACTION_EVENT_TICKS.saturating_sub(elapsed));
    i32::from(distance) * 3
}

fn render_battle_event(renderer: &mut Renderer, battle: &BattleState, event: BattleEvent) {
    let (x, y, damage) = match event {
        BattleEvent::PlayerAttack { enemy, damage, .. }
        | BattleEvent::PlayerMagic { enemy, damage, .. }
        | BattleEvent::SimulatedMagic { enemy, damage, .. } => {
            let Some(enemy) = battle.enemies.get(enemy) else {
                return;
            };
            (
                i32::from(enemy.position.x) + 12,
                i32::from(enemy.position.y) - 24,
                damage,
            )
        }
        BattleEvent::EnemyAttack { player, damage, .. }
        | BattleEvent::EnemyMagic { player, damage, .. }
        | BattleEvent::PlayerConfusedAttack {
            target: player,
            damage,
            ..
        } => {
            let (x, y) = player_position(battle.players.len(), player);
            (x + 12, y - 34, damage)
        }
        BattleEvent::EnemyConfusedAttack { target, damage, .. } => {
            let Some(enemy) = battle.enemies.get(target) else {
                return;
            };
            (
                i32::from(enemy.position.x) + 12,
                i32::from(enemy.position.y) - 24,
                damage,
            )
        }
        BattleEvent::PlayerUseItem { .. }
        | BattleEvent::PlayerThrowItem { .. }
        | BattleEvent::RoundCompleted
        | BattleEvent::Finished(_) => return,
    };
    draw_debug_text(renderer, x - 28, y, "DMG", [255, 248, 160, 255]);
    draw_number(
        renderer,
        u32::from(damage),
        x + 8,
        y + 1,
        [255, 255, 255, 255],
    );
}

fn render_settlement(renderer: &mut Renderer, battle: &BattleState, result: BattleResult) {
    fill_rect(renderer, 80, 58, 160, 76, [8, 16, 32, 255]);
    stroke_rect(renderer, 80, 58, 160, 76, [255, 236, 80, 255]);
    let title = match result {
        BattleResult::Won => "VICTORY",
        BattleResult::Lost => "DEFEAT",
        BattleResult::Fled => "FLED",
        BattleResult::Terminated => "ENDED",
    };
    draw_debug_text(
        renderer,
        160 - i32::try_from(title.len()).unwrap_or(0) * 3,
        68,
        title,
        [255, 236, 80, 255],
    );
    if result == BattleResult::Won {
        let rewards = battle.rewards();
        draw_debug_text(renderer, 104, 88, "EXP", [120, 220, 255, 255]);
        draw_number(renderer, rewards.experience, 216, 89, [255, 255, 255, 255]);
        draw_debug_text(renderer, 104, 101, "CASH", [120, 220, 255, 255]);
        draw_number(renderer, rewards.cash, 216, 102, [255, 255, 255, 255]);
    }
    draw_debug_text(renderer, 130, 120, "ENTER", [200, 208, 220, 255]);
}

fn render_status(
    renderer: &mut Renderer,
    battle: &BattleState,
    text: &TextLibrary,
    font: &BitmapFont,
    selected_command: usize,
) {
    let count = battle.players.len().max(1);
    let width = (310 / i32::try_from(count).unwrap_or(1)).clamp(64, 100);
    let active = battle.active_player();
    for (index, player) in battle.players.iter().enumerate() {
        let x = 5 + i32::try_from(index).unwrap_or(0) * width;
        let y = 164;
        fill_rect(renderer, x, y, width - 3, 35, [8, 16, 32, 255]);
        stroke_rect(
            renderer,
            x,
            y,
            width - 3,
            35,
            if active == Some(index) {
                [255, 236, 80, 255]
            } else {
                [120, 160, 200, 255]
            },
        );
        if let Some(name) = text.word(usize::from(player.name_word_id)) {
            renderer.draw_big5_text(font, name, x + 3, y + 1, 0x2d);
        }
        draw_debug_text(renderer, x + 3, y + 18, "HP", [120, 220, 255, 255]);
        draw_number(
            renderer,
            u32::from(player.hp),
            x + width - 8,
            y + 19,
            [255, 236, 80, 255],
        );
        draw_debug_text(renderer, x + 3, y + 27, "MP", [120, 220, 255, 255]);
        draw_number(
            renderer,
            u32::from(player.mp),
            x + width - 8,
            y + 28,
            [160, 220, 255, 255],
        );
    }
    draw_debug_text(
        renderer,
        278,
        188,
        battle_command_label(selected_command),
        [255, 255, 255, 255],
    );
}

fn battle_command_label(selected: usize) -> &'static str {
    ["ATK", "MAG", "USE", "THR"]
        .get(selected)
        .copied()
        .unwrap_or("ATK")
}

fn draw_selector(renderer: &mut Renderer, x: i32, y: i32, color: [u8; 4]) {
    for row in 0..5 {
        for column in -row..=row {
            renderer.put_rgba(x + column, y + row, color);
        }
    }
}

fn player_position(count: usize, index: usize) -> (i32, i32) {
    const POSITIONS: [[(i32, i32); 3]; 3] = [
        [(240, 170), (240, 170), (240, 170)],
        [(200, 176), (256, 152), (256, 152)],
        [(180, 180), (234, 170), (270, 146)],
    ];
    if (1..=3).contains(&count) && index < count {
        return POSITIONS[count - 1][index];
    }
    (
        180 + i32::try_from(index).unwrap_or(0) * 28,
        180 - i32::try_from(index).unwrap_or(0) * 9,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_positions_match_original_one_to_three_member_layouts() {
        assert_eq!(player_position(1, 0), (240, 170));
        assert_eq!(player_position(2, 0), (200, 176));
        assert_eq!(player_position(2, 1), (256, 152));
        assert_eq!(player_position(3, 2), (270, 146));
        assert_eq!(player_position(4, 3), (264, 153));
    }

    #[test]
    fn action_offset_moves_out_and_returns_to_idle_position() {
        let offsets = (1..=ACTION_EVENT_TICKS)
            .rev()
            .map(action_offset)
            .collect::<Vec<_>>();
        assert_eq!(offsets, [0, 3, 6, 9, 12, 9, 6, 3]);
    }

    #[test]
    fn battle_command_labels_cover_attack_magic_use_and_throw() {
        assert_eq!(
            (0..4).map(battle_command_label).collect::<Vec<_>>(),
            ["ATK", "MAG", "USE", "THR"]
        );
    }
}
