use std::path::PathBuf;

use pal_desktop::audio::MusicBackend;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommandMode {
    Run,
    CheckAssets,
    SceneEditor { scene: u16 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Command {
    pub(super) mode: CommandMode,
    pub(super) music_backend: MusicBackend,
    pub(super) sound_font_path: Option<PathBuf>,
}

impl Command {
    pub(super) fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut command = Self {
            mode: CommandMode::Run,
            music_backend: MusicBackend::Midi,
            sound_font_path: None,
        };
        let mut music_backend_seen = false;
        let mut scene_editor_seen = false;
        let mut scene_number = None;

        for argument in arguments {
            if argument == "--check-assets" {
                if command.mode != CommandMode::Run {
                    return Err("only one command mode may be specified".into());
                }
                command.mode = CommandMode::CheckAssets;
            } else if argument == "--scene-editor" {
                if command.mode != CommandMode::Run || scene_editor_seen {
                    return Err("only one command mode may be specified".into());
                }
                scene_editor_seen = true;
                command.mode = CommandMode::SceneEditor { scene: 1 };
            } else if let Some(value) = argument.strip_prefix("--scene=") {
                if scene_number.is_some() {
                    return Err("--scene was specified more than once".into());
                }
                let scene = value
                    .parse::<u16>()
                    .ok()
                    .filter(|scene| *scene != 0)
                    .ok_or_else(|| format!("invalid scene number: {value}"))?;
                scene_number = Some(scene);
            } else if let Some(value) = argument.strip_prefix("--music=") {
                if music_backend_seen {
                    return Err("--music was specified more than once".into());
                }
                command.music_backend = match value {
                    "midi" => MusicBackend::Midi,
                    "rix" => MusicBackend::Rix,
                    _ => return Err(format!("invalid music backend: {value}")),
                };
                music_backend_seen = true;
            } else if let Some(value) = argument.strip_prefix("--sound-font=") {
                if command.sound_font_path.is_some() {
                    return Err("--sound-font was specified more than once".into());
                }
                if value.is_empty() {
                    return Err("--sound-font requires a path".into());
                }
                command.sound_font_path = Some(PathBuf::from(value));
            } else {
                return Err(format!("unknown argument: {argument}"));
            }
        }
        if let Some(scene) = scene_number {
            if !scene_editor_seen {
                return Err("--scene requires --scene-editor".into());
            }
            command.mode = CommandMode::SceneEditor { scene };
        }
        if matches!(command.mode, CommandMode::SceneEditor { .. })
            && (music_backend_seen || command.sound_font_path.is_some())
        {
            return Err("music options are not available in scene-editor mode".into());
        }
        Ok(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_run_and_asset_check_commands() {
        assert_eq!(
            Command::parse([]).unwrap(),
            Command {
                mode: CommandMode::Run,
                music_backend: MusicBackend::Midi,
                sound_font_path: None,
            }
        );
        assert_eq!(
            Command::parse(["--check-assets".to_string()]).unwrap().mode,
            CommandMode::CheckAssets
        );
        assert_eq!(
            Command::parse(["--scene-editor".to_string(), "--scene=7".to_string()])
                .unwrap()
                .mode,
            CommandMode::SceneEditor { scene: 7 }
        );
        assert_eq!(
            Command::parse(["--scene-editor".to_string()]).unwrap().mode,
            CommandMode::SceneEditor { scene: 1 }
        );
        assert!(Command::parse(["--unknown".to_string()]).is_err());
        assert!(
            Command::parse(["--check-assets".to_string(), "--check-assets".to_string()]).is_err()
        );
        assert!(Command::parse(["--scene=7".to_string()]).is_err());
        assert!(
            Command::parse(["--scene-editor".to_string(), "--check-assets".to_string()]).is_err()
        );
        assert!(Command::parse(["--scene-editor".to_string(), "--scene=0".to_string()]).is_err());
    }

    #[test]
    fn parses_music_backend_and_sound_font() {
        let command = Command::parse([
            "--music=rix".to_string(),
            "--sound-font=/tmp/custom.sf2".to_string(),
        ])
        .unwrap();
        assert_eq!(command.music_backend, MusicBackend::Rix);
        assert_eq!(
            command.sound_font_path,
            Some(PathBuf::from("/tmp/custom.sf2"))
        );
        assert!(Command::parse(["--music=other".to_string()]).is_err());
        assert!(Command::parse(["--sound-font=".to_string()]).is_err());
        assert!(Command::parse(["--music=midi".to_string(), "--music=rix".to_string()]).is_err());
        assert!(Command::parse(["--scene-editor".to_string(), "--music=rix".to_string()]).is_err());
    }
}
