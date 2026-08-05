use pal_assets::battle::BattleSpriteArchive;
use pal_assets::bitmap::Bitmap;
use pal_assets::rle::RleBitmap;
use pal_assets::text::{BitmapFont, TextLibrary};
use pal_core::battle::{BattleEvent, BattlePhase, BattleResult, BattleState};

use super::battle_update::{ACTION_EVENT_TICKS, PLAYER_MAGIC_ANIMATION_EVENT_TICKS};
use super::draw::{draw_debug_text, draw_number, fill_rect, stroke_rect};
use crate::renderer::Renderer;

const BATTLE_COMMAND_ICONS: [(usize, i32, i32); 4] =
    [(40, 27, 140), (41, 0, 155), (42, 54, 155), (43, 27, 170)];

pub struct BattleRenderResources<'a> {
    pub enemy_sprites: &'a BattleSpriteArchive,
    pub player_sprites: &'a BattleSpriteArchive,
    pub backgrounds: &'a [Option<Bitmap>],
    pub text: &'a TextLibrary,
    pub font: &'a BitmapFont,
    pub ui_sprites: &'a [RleBitmap],
}

#[derive(Clone, Copy)]
pub struct BattleRenderState {
    pub selected_enemy: usize,
    pub selected_command: usize,
    pub targeting_enemy: bool,
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
        targeting_enemy,
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
        let enemy_action_offset = match event {
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
        let enemy_blow_offset = match event {
            Some(
                BattleEvent::PlayerMagic { blow, .. } | BattleEvent::SimulatedMagic { blow, .. },
            ) => magic_blow_offset(blow, event_ticks),
            _ => 0,
        };
        let enemy_offset = enemy_action_offset + enemy_blow_offset;
        let x = i32::from(enemy.position.x) - i32::from(bitmap.width) / 2 + enemy_offset;
        let y = i32::from(enemy.position.y) + i32::from(enemy.y_offset) - i32::from(bitmap.height)
            + enemy_offset / 2;
        if event.is_none()
            && targeting_enemy
            && index == selected_enemy
            && battle.phase() == BattlePhase::AwaitingCommand
            && ticks & 1 != 0
        {
            renderer.blit_rle_color_shift(&bitmap, x, y, 7);
        } else {
            renderer.blit_rle(&bitmap, x, y);
        }
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
    }

    for (index, player) in battle.players.iter().enumerate() {
        let (mut x, mut y) = player_position(battle.players.len(), index);
        let (magic_x, magic_y, magic_frame, color_shift) =
            player_magic_animation_state(event, index, event_ticks);
        x += magic_x;
        y += magic_y;
        if let Some(BattleEvent::EnemyMagic { blow, .. }) = event {
            let offset = magic_blow_offset(blow, event_ticks);
            x += offset;
            y += offset / 2;
        }
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
        let frame = magic_frame.unwrap_or_else(|| {
            if player.is_alive() {
                0
            } else {
                2.min(available.saturating_sub(1))
            }
        });
        let frame = frame.min(available.saturating_sub(1));
        if let Some(bitmap) = resources.player_sprites.decode_frame(sprite, frame) {
            let left = x - i32::from(bitmap.width) / 2;
            let top = y - i32::from(bitmap.height);
            if color_shift == 0 {
                renderer.blit_rle(&bitmap, left, top);
            } else {
                renderer.blit_rle_color_shift(&bitmap, left, top, color_shift);
            }
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

    if event.is_none() && battle.phase() == BattlePhase::AwaitingCommand {
        render_status(
            renderer,
            battle,
            resources.ui_sprites,
            selected_command,
            targeting_enemy,
            ticks,
        );
    }
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

fn magic_blow_offset(amount: i16, ticks_remaining: u16) -> i32 {
    if amount == 0 {
        return 0;
    }
    let frame_count = ACTION_EVENT_TICKS
        .saturating_sub(ticks_remaining.min(ACTION_EVENT_TICKS))
        .saturating_add(1);
    let lower = i32::from(amount.min(0));
    let upper = i32::from(amount.max(0));
    let span = u32::try_from(upper - lower + 1).unwrap_or(1);
    let mut state = 0x6d2b_79f5 ^ u32::from(amount as u16);
    let mut offset = 0i32;
    for _ in 0..frame_count {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        offset += lower + i32::try_from(state % span).unwrap_or(0);
    }
    offset
}

fn player_magic_animation_state(
    event: Option<BattleEvent>,
    player_index: usize,
    ticks_remaining: u16,
) -> (i32, i32, Option<usize>, i16) {
    let Some(BattleEvent::PlayerMagicAnimation { player }) = event else {
        return (0, 0, None, 0);
    };
    let elapsed = PLAYER_MAGIC_ANIMATION_EVENT_TICKS
        .saturating_sub(ticks_remaining.min(PLAYER_MAGIC_ANIMATION_EVENT_TICKS));
    let color_shift = if elapsed >= 17 {
        i16::try_from((elapsed - 17).min(4) * 2).unwrap_or(8)
    } else {
        0
    };
    if player != Some(player_index) {
        return (0, 0, None, color_shift);
    }
    let movement_steps = usize::from(elapsed.saturating_add(1).min(4));
    let x = -[4, 3, 2, 1][..movement_steps].iter().sum::<i32>();
    let y = -[2, 1, 1, 0][..movement_steps].iter().sum::<i32>();
    let frame = if elapsed >= 17 {
        6
    } else if elapsed >= 6 {
        5
    } else {
        0
    };
    (x, y, Some(frame), color_shift)
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
        | BattleEvent::PlayerFlee { .. }
        | BattleEvent::PlayerMagicAnimation { .. }
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
    ui_sprites: &[RleBitmap],
    selected_command: usize,
    targeting_enemy: bool,
    ticks: u64,
) {
    for (index, player) in battle.players.iter().enumerate() {
        let x = 91 + i32::try_from(index).unwrap_or(0) * 77;
        let y = 165;
        if let Some(info_box) = ui_sprites.get(18) {
            renderer.blit_rle(info_box, x, y);
        }
        if let Some(face) = ui_sprites.get(48 + usize::from(player.role_id)) {
            renderer.blit_rle(face, x - 2, y - 4);
        }
        if let Some(slash) = ui_sprites.get(39) {
            renderer.blit_rle(slash, x + 49, y + 6);
            renderer.blit_rle(slash, x + 49, y + 22);
        }
        draw_ui_number(
            renderer,
            ui_sprites,
            u32::from(player.hp),
            4,
            x + 26,
            y + 5,
            19,
            [240, 224, 96, 255],
        );
        draw_ui_number(
            renderer,
            ui_sprites,
            u32::from(player.max_hp),
            4,
            x + 47,
            y + 8,
            19,
            [240, 224, 96, 255],
        );
        draw_ui_number(
            renderer,
            ui_sprites,
            u32::from(player.mp),
            4,
            x + 26,
            y + 21,
            56,
            [96, 224, 240, 255],
        );
        draw_ui_number(
            renderer,
            ui_sprites,
            u32::from(player.max_mp),
            4,
            x + 47,
            y + 24,
            56,
            [96, 224, 240, 255],
        );
    }

    if targeting_enemy {
        return;
    }
    if let Some(active) = battle.active_player() {
        let (x, y) = player_position(battle.players.len(), active);
        let arrow = if ticks & 1 == 0 { 68 } else { 69 };
        if let Some(sprite) = ui_sprites.get(arrow) {
            renderer.blit_rle(sprite, x - 8, y - 74);
        }
    }
    for (index, (sprite_index, x, y)) in BATTLE_COMMAND_ICONS.into_iter().enumerate() {
        let Some(sprite) = ui_sprites.get(sprite_index) else {
            continue;
        };
        if index == selected_command {
            renderer.blit_rle(sprite, x, y);
        } else {
            renderer.blit_rle_mono(sprite, x, y, 0, -4);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_ui_number(
    renderer: &mut Renderer,
    ui_sprites: &[RleBitmap],
    value: u32,
    length: usize,
    x: i32,
    y: i32,
    sprite_base: usize,
    fallback: [u8; 4],
) {
    if ui_sprites.get(sprite_base + 9).is_none() {
        draw_number(renderer, value, x + length as i32 * 6, y, fallback);
        return;
    }
    let digits = value.to_string();
    let visible = &digits[digits.len().saturating_sub(length)..];
    let mut draw_x = x + (length.saturating_sub(visible.len())) as i32 * 6;
    for digit in visible.bytes() {
        renderer.blit_rle(
            &ui_sprites[sprite_base + usize::from(digit - b'0')],
            draw_x,
            y,
        );
        draw_x += 6;
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
    fn magic_blow_accumulates_with_the_requested_sign_and_resets_after_feedback() {
        let negative = (1..=ACTION_EVENT_TICKS)
            .rev()
            .map(|ticks| magic_blow_offset(-3, ticks))
            .collect::<Vec<_>>();
        let positive = (1..=ACTION_EVENT_TICKS)
            .rev()
            .map(|ticks| magic_blow_offset(2, ticks))
            .collect::<Vec<_>>();
        assert!(negative.windows(2).all(|pair| pair[1] <= pair[0]));
        assert!(positive.windows(2).all(|pair| pair[1] >= pair[0]));
        assert!(negative.last().is_some_and(|&offset| offset < 0));
        assert!(positive.last().is_some_and(|&offset| offset > 0));
        assert_eq!(magic_blow_offset(0, 1), 0);
    }

    #[test]
    fn scripted_magic_animation_moves_selected_player_then_shifts_entire_party() {
        let event = Some(BattleEvent::PlayerMagicAnimation { player: Some(1) });
        assert_eq!(
            player_magic_animation_state(event, 1, 22),
            (-4, -2, Some(0), 0)
        );
        assert_eq!(
            player_magic_animation_state(event, 1, 16),
            (-10, -4, Some(5), 0)
        );
        assert_eq!(
            player_magic_animation_state(event, 1, 5),
            (-10, -4, Some(6), 0)
        );
        assert_eq!(player_magic_animation_state(event, 0, 1), (0, 0, None, 8));
        assert_eq!(
            player_magic_animation_state(
                Some(BattleEvent::PlayerMagicAnimation { player: None }),
                0,
                1,
            ),
            (0, 0, None, 8)
        );
    }

    #[test]
    fn battle_command_icons_match_the_original_cross_layout() {
        assert_eq!(
            BATTLE_COMMAND_ICONS,
            [(40, 27, 140), (41, 0, 155), (42, 54, 155), (43, 27, 170)]
        );
    }
}
