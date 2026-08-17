//! The command line. Parsed by hand: four flags do not justify a
//! dependency, and the parser being a pure function means the venue
//! mistakes — a typo in a monitor index, a missing file — have tests.

use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct Cli {
    /// The `.animus` project directory to open. `None` starts untitled.
    pub project: Option<PathBuf>,
    /// Boot straight to the output window with no editor. Spec §18 M2 makes
    /// this fast; M1 only promises that the flag exists and is honoured.
    pub perform: bool,
    /// Explicit output monitor index.
    pub output_monitor: Option<usize>,
    /// Start with output vsync off.
    pub no_vsync: bool,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CliError {
    #[error("--output-monitor needs a number, got {0:?}")]
    BadMonitorIndex(String),
    #[error("unknown flag {0:?} (known: --perform, --output-monitor <n>, --no-vsync)")]
    UnknownFlag(String),
    #[error("more than one project path given: {0:?} and {1:?}")]
    TwoProjects(PathBuf, PathBuf),
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Cli, CliError> {
    let mut cli = Cli {
        project: None,
        perform: false,
        output_monitor: None,
        no_vsync: false,
    };

    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--perform" => cli.perform = true,
            "--no-vsync" => cli.no_vsync = true,
            "--output-monitor" => {
                let value = args.next().unwrap_or_default();
                cli.output_monitor = Some(
                    value
                        .parse()
                        .map_err(|_| CliError::BadMonitorIndex(value))?,
                );
            }
            flag if flag.starts_with("--") => {
                return Err(CliError::UnknownFlag(flag.to_string()));
            }
            path => {
                let path = PathBuf::from(path);
                if let Some(existing) = cli.project.replace(path.clone()) {
                    return Err(CliError::TwoProjects(existing, path));
                }
            }
        }
    }
    Ok(cli)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(s: &str) -> Result<Cli, CliError> {
        parse(s.split_whitespace().map(String::from))
    }

    #[test]
    fn no_arguments_starts_untitled() {
        let cli = parse_str("").unwrap();
        assert_eq!(cli.project, None);
        assert!(!cli.perform);
    }

    #[test]
    fn a_project_and_flags_in_any_order() {
        let cli = parse_str("show.animus --no-vsync --output-monitor 2").unwrap();
        assert_eq!(cli.project, Some(PathBuf::from("show.animus")));
        assert!(cli.no_vsync);
        assert_eq!(cli.output_monitor, Some(2));

        let cli = parse_str("--perform show.animus").unwrap();
        assert!(cli.perform);
        assert_eq!(cli.project, Some(PathBuf::from("show.animus")));
    }

    #[test]
    fn a_bad_monitor_index_names_itself() {
        let err = parse_str("--output-monitor projector").unwrap_err();
        assert_eq!(err, CliError::BadMonitorIndex("projector".into()));
        let err = parse_str("--output-monitor").unwrap_err();
        assert_eq!(err, CliError::BadMonitorIndex(String::new()));
    }

    #[test]
    fn an_unknown_flag_is_an_error_not_a_project_path() {
        // `animus --preform show.animus` must fail loudly, not open an
        // untitled project named nothing while silently dropping the typo.
        let err = parse_str("--preform show.animus").unwrap_err();
        assert!(matches!(err, CliError::UnknownFlag(_)));
    }

    #[test]
    fn two_project_paths_are_refused() {
        let err = parse_str("one.animus two.animus").unwrap_err();
        assert!(matches!(err, CliError::TwoProjects(..)));
    }
}
