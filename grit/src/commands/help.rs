//! `git help` — display help information compatible with Git's builtin.
//!
//! User-facing strings say `git` because the binary is invoked as `git` in tests.

use anyhow::Result;
use clap::Args as ClapArgs;
use std::io::{self, Write};
use std::path::Path;

/// Config variable names for completion (from `git help --config-for-completion`).
const CONFIG_VARS_FOR_COMPLETION: &str = include_str!("config_vars.txt");

/// Config section names for completion (from `git help --config-sections-for-completion`).
const CONFIG_SECTIONS_FOR_COMPLETION: &str = include_str!("config_sections.txt");

/// Full config variable names with placeholders (from `git help --config`).
const CONFIG_VARS_ALL: &str = include_str!("config_vars_all.txt");

const COMMON_HELP: &str = include_str!("help/assets/common_help.txt");
const ALL_COMMANDS_HELP: &str = include_str!("help/assets/all_commands_help.txt");
const GUIDES_HELP: &str = include_str!("help/assets/guides_help.txt");
const USER_INTERFACES_HELP: &str = include_str!("help/assets/user_interfaces_help.txt");
const DEVELOPER_INTERFACES_HELP: &str = include_str!("help/assets/developer_interfaces_help.txt");

/// Usage line printed for `git help -a --no-verbose` (matches Git's `git_usage_string`).
const GIT_USAGE_LINE: &str = "usage: git [-v | --version] [-h | --help] [-C <path>] [-c <name>=<value>]\n           [--exec-path[=<path>]] [--html-path] [--man-path] [--info-path]\n           [-p | --paginate | -P | --no-pager] [--no-replace-objects] [--no-lazy-fetch]\n           [--no-optional-locks] [--no-advice] [--bare] [--git-dir=<path>]\n           [--work-tree=<path>] [--namespace=<name>] [--config-env=<name>=<envvar>]\n           <command> [<args>]\n";

/// Arguments for `git help`.
#[derive(Debug, ClapArgs)]
#[command(about = "Display help information")]
pub struct Args {
    /// List all available commands (verbose listing by default).
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    #[arg(long = "no-verbose")]
    pub no_verbose: bool,

    #[arg(long = "verbose")]
    pub verbose: bool,

    #[arg(long = "no-external-commands")]
    pub no_external_commands: bool,

    #[arg(long = "external-commands", hide = true)]
    pub external_commands: bool,

    #[arg(long = "no-aliases")]
    pub no_aliases: bool,

    #[arg(long = "aliases", hide = true)]
    pub aliases: bool,

    #[arg(long = "exclude-guides", hide = true)]
    pub exclude_guides: bool,

    #[arg(short = 'g', long = "guides")]
    pub guides: bool,

    #[arg(short = 'c', long = "config")]
    pub list_config: bool,

    #[arg(long = "user-interfaces")]
    pub user_interfaces: bool,

    #[arg(long = "developer-interfaces")]
    pub developer_interfaces: bool,

    #[arg(long = "config-for-completion", hide = true)]
    pub config_for_completion: bool,

    #[arg(long = "config-sections-for-completion", hide = true)]
    pub config_sections_for_completion: bool,

    #[arg(short = 'i', long = "info")]
    pub info: bool,

    #[arg(short = 'm', long = "man")]
    pub man: bool,

    #[arg(short = 'w', long = "web")]
    pub web: bool,

    /// Command or guide to show documentation for.
    pub command: Option<String>,
}

/// Print the same text as `git help` with no arguments (also used for bare `git`).
pub fn print_common_help() {
    print!("{COMMON_HELP}");
}

/// Run `git help`.
pub fn run(args: Args) -> Result<()> {
    if args.verbose && args.no_verbose {
        std::process::exit(129);
    }

    if (args.no_external_commands || args.external_commands || args.no_aliases || args.aliases)
        && !args.all
    {
        std::process::exit(129);
    }

    let format_flags = (args.info as u8) + (args.man as u8) + (args.web as u8);
    if format_flags > 1 {
        std::process::exit(129);
    }

    let list_mode_count = [
        args.all,
        args.guides,
        args.list_config,
        args.user_interfaces,
        args.developer_interfaces,
        args.config_for_completion,
        args.config_sections_for_completion,
    ]
    .into_iter()
    .filter(|b| *b)
    .count();

    if list_mode_count > 1 {
        std::process::exit(129);
    }

    let list_mode_active = list_mode_count == 1;
    let help_format_flag = if args.info {
        HelpFormat::Info
    } else if args.man {
        HelpFormat::Man
    } else if args.web {
        HelpFormat::Html
    } else {
        HelpFormat::None
    };

    if list_mode_active && help_format_flag != HelpFormat::None {
        std::process::exit(129);
    }

    if list_mode_active && args.command.is_some() {
        std::process::exit(129);
    }

    if args.config_for_completion {
        if help_format_flag != HelpFormat::None {
            std::process::exit(129);
        }
        print!("{}", CONFIG_VARS_FOR_COMPLETION);
        return Ok(());
    }

    if args.config_sections_for_completion {
        if help_format_flag != HelpFormat::None {
            std::process::exit(129);
        }
        print!("{}", CONFIG_SECTIONS_FOR_COMPLETION);
        return Ok(());
    }

    if args.list_config {
        if help_format_flag != HelpFormat::None {
            std::process::exit(129);
        }
        print!("{}", CONFIG_VARS_ALL);
        return Ok(());
    }

    if args.user_interfaces {
        if help_format_flag != HelpFormat::None {
            std::process::exit(129);
        }
        print!("{}", USER_INTERFACES_HELP);
        return Ok(());
    }

    if args.developer_interfaces {
        if help_format_flag != HelpFormat::None {
            std::process::exit(129);
        }
        print!("{}", DEVELOPER_INTERFACES_HELP);
        return Ok(());
    }

    if args.guides {
        if help_format_flag != HelpFormat::None {
            std::process::exit(129);
        }
        print!("{}", GUIDES_HELP);
        return Ok(());
    }

    if args.all {
        if help_format_flag != HelpFormat::None {
            std::process::exit(129);
        }
        let verbose = !args.no_verbose;
        if verbose {
            print!("{}", ALL_COMMANDS_HELP);
        } else {
            print_all_commands_no_verbose()?;
        }
        return Ok(());
    }

    if args.command.is_none() {
        print_common_help();
        return Ok(());
    }

    let cmd = args.command.as_deref().unwrap_or("");
    if args.exclude_guides && !is_known_command(cmd) {
        eprintln!("git: '{cmd}' is not a git command. See 'git --help'.");
        std::process::exit(1);
    }

    show_documentation_for_command(cmd, help_format_flag)
}

fn is_known_command(cmd: &str) -> bool {
    crate::KNOWN_COMMANDS.contains(&cmd)
}

fn cmd_to_page(cmd: &str) -> String {
    if cmd.starts_with("git") {
        return cmd.to_string();
    }
    if cmd == "scalar" {
        return "scalar".to_string();
    }
    if is_known_command(cmd) {
        return format!("git-{cmd}");
    }
    format!("git{cmd}")
}

fn show_documentation_for_command(cmd: &str, help_format_flag: HelpFormat) -> Result<()> {
    let config = match grit_lib::repo::Repository::discover(None) {
        Ok(repo) => grit_lib::config::ConfigSet::load(Some(&repo.git_dir), true).ok(),
        Err(_) => None,
    };

    let mut help_format = help_format_flag;
    if help_format == HelpFormat::None {
        if let Some(ref cfg) = config {
            if let Some(v) = cfg.get("help.format") {
                help_format = parse_help_format(v.as_str()).unwrap_or(HelpFormat::Man);
            } else {
                help_format = HelpFormat::Man;
            }
        } else {
            help_format = HelpFormat::Man;
        }
    }

    let page = cmd_to_page(cmd);

    match help_format {
        HelpFormat::Html => {
            let Some(cfg) = config else {
                eprintln!("git: could not read configuration for HTML help");
                std::process::exit(1);
            };
            let htmlpath = cfg.get("help.htmlpath").unwrap_or_default();
            let full_path = format!("{htmlpath}/{page}.html");
            if !htmlpath.contains("://") {
                let p = Path::new(&htmlpath).join(format!("{page}.html"));
                if !p.is_file() {
                    eprintln!(
                        "'{}/{}.html': documentation file not found.",
                        htmlpath.trim_end_matches('/'),
                        page
                    );
                    std::process::exit(1);
                }
            }
            open_html_browser(&cfg, &full_path)?;
        }
        HelpFormat::Man => {
            let status = std::process::Command::new("man")
                .arg(&page)
                .status()
                .map_err(|e| anyhow::anyhow!("failed to run man: {e}"))?;
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
        HelpFormat::Info => {
            let status = std::process::Command::new("info")
                .args(["gitman", &page])
                .status()
                .map_err(|e| anyhow::anyhow!("failed to run info: {e}"))?;
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
        HelpFormat::None => {
            let status = std::process::Command::new("man")
                .arg(&page)
                .status()
                .map_err(|e| anyhow::anyhow!("failed to run man: {e}"))?;
            if !status.success() {
                std::process::exit(status.code().unwrap_or(1));
            }
        }
    }

    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HelpFormat {
    None,
    Man,
    Info,
    Html,
}

fn parse_help_format(s: &str) -> Option<HelpFormat> {
    match s {
        "man" => Some(HelpFormat::Man),
        "info" => Some(HelpFormat::Info),
        "web" | "html" => Some(HelpFormat::Html),
        _ => None,
    }
}

fn open_html_browser(config: &grit_lib::config::ConfigSet, path: &str) -> Result<()> {
    let tool = config
        .get("help.browser")
        .or_else(|| config.get("web.browser"))
        .unwrap_or_else(|| "firefox".to_string());
    let key = format!("browser.{tool}.cmd");
    let Some(cmd_str) = config.get(&key) else {
        eprintln!("git: browser.{tool}.cmd not configured for HTML help");
        std::process::exit(1);
    };

    let parts: Vec<&str> = cmd_str.split_whitespace().collect();
    if parts.is_empty() {
        eprintln!("git: empty browser.{tool}.cmd");
        std::process::exit(1);
    }

    let status = std::process::Command::new(parts[0])
        .args(&parts[1..])
        .arg(path)
        .status()
        .map_err(|e| anyhow::anyhow!("failed to run browser command: {e}"))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn print_all_commands_no_verbose() -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();
    writeln!(out, "{GIT_USAGE_LINE}")?;
    let exec_path = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.display().to_string()))
        .unwrap_or_else(|| ".".to_string());
    writeln!(out, "available git commands in '{exec_path}'")?;
    writeln!(out)?;

    let mut names: Vec<&str> = crate::KNOWN_COMMANDS.to_vec();
    names.sort();
    let col_width = 25usize;
    let mid = names.len().div_ceil(2);
    for i in 0..mid {
        let left = names[i];
        write!(out, "  {left:width$}", width = col_width)?;
        if let Some(right) = names.get(i + mid) {
            writeln!(out, "{right}")?;
        } else {
            writeln!(out)?;
        }
    }
    writeln!(out)?;
    writeln!(
        out,
        "See 'git help <command>' or 'git <command> --help' for more information."
    )?;
    Ok(())
}
