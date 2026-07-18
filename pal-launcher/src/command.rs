#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Command {
    Run,
    CheckAssets,
}

impl Command {
    pub(super) fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut command = Self::Run;
        for argument in arguments {
            match argument.as_str() {
                "--check-assets" if command == Self::Run => command = Self::CheckAssets,
                "--check-assets" => {
                    return Err("--check-assets was specified more than once".into())
                }
                _ => return Err(format!("unknown argument: {argument}")),
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
        assert_eq!(Command::parse([]).unwrap(), Command::Run);
        assert_eq!(
            Command::parse(["--check-assets".to_string()]).unwrap(),
            Command::CheckAssets
        );
        assert!(Command::parse(["--unknown".to_string()]).is_err());
        assert!(
            Command::parse(["--check-assets".to_string(), "--check-assets".to_string()]).is_err()
        );
    }
}
