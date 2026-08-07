use std::path::PathBuf;

use pal_desktop::audio::MusicBackend;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommandMode {
    Run,
    CheckAssets,
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

        for argument in arguments {
            if argument == "--check-assets" {
                if command.mode == CommandMode::CheckAssets {
                    return Err("--check-assets was specified more than once".into());
                }
                command.mode = CommandMode::CheckAssets;
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
        assert!(Command::parse(["--unknown".to_string()]).is_err());
        assert!(
            Command::parse(["--check-assets".to_string(), "--check-assets".to_string()]).is_err()
        );
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
    }
}
