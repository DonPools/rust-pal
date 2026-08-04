use std::path::Path;

use pal_core::game::GameState;
use pal_core::role::RoleSprites;

use super::LoadedScene;

pub(super) enum RestoreSnapshotError {
    Unavailable,
    SceneUnavailable,
}

pub(super) fn save_snapshot(path: &Path, game: &GameState) -> std::io::Result<()> {
    let bytes = game
        .encode_snapshot()
        .ok_or_else(|| std::io::Error::other("failed to encode snapshot"))?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, bytes)?;
    match std::fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(_error) if path.exists() => {
            std::fs::remove_file(path)?;
            std::fs::rename(temporary, path)
        }
        Err(error) => Err(error),
    }
}

pub(super) fn restore_snapshot<L>(
    path: &Path,
    game: &mut GameState,
    role_sprites: &RoleSprites,
    load_scene: &mut L,
) -> Result<(), RestoreSnapshotError>
where
    L: FnMut(u16, Option<u16>, &RoleSprites) -> Option<LoadedScene>,
{
    let bytes = std::fs::read(path).map_err(|_| RestoreSnapshotError::Unavailable)?;
    let snapshot = game
        .decode_snapshot(&bytes)
        .ok_or(RestoreSnapshotError::Unavailable)?;
    let scene = load_scene(
        snapshot.scene_number(),
        snapshot.scene_map_override(),
        role_sprites,
    )
    .ok_or(RestoreSnapshotError::SceneUnavailable)?;
    game.restore_snapshot(snapshot, scene.map);
    Ok(())
}
