//! `grit help` — display help information.
//!
//! Lists available commands, guides, interfaces, and command-specific help.

use anyhow::{bail, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::repo::Repository;
use std::io::{self, Write};
use std::path::Path;

/// Config variable names for completion (from `git help --config-for-completion`).
const CONFIG_VARS_FOR_COMPLETION: &str = include_str!("config_vars.txt");

/// Config section names for completion (from `git help --config-sections-for-completion`).
const CONFIG_SECTIONS_FOR_COMPLETION: &str = include_str!("config_sections.txt");

/// Full config variable names with placeholders (from `git help --config`).
const CONFIG_VARS_ALL: &str = include_str!("config_vars_all.txt");

/// Arguments for `grit help`.
#[derive(Debug, ClapArgs)]
#[command(about = "Display help information")]
pub struct Args {
    /// List all available commands.
    #[arg(short = 'a', long = "all")]
    pub all: bool,

    /// Print list of concept guides.
    #[arg(short = 'g', long = "guides")]
    pub guides: bool,

    /// Print list of user-facing interfaces.
    #[arg(long = "user-interfaces")]
    pub user_interfaces: bool,

    /// List config variable names for completion.
    #[arg(long = "config-for-completion", hide = true)]
    pub config_for_completion: bool,

    /// List all config variable names.
    #[arg(short = 'c', long = "config", hide = true)]
    pub config_list: bool,

    /// List config section names for completion.
    #[arg(long = "config-sections-for-completion", hide = true)]
    pub config_sections_for_completion: bool,

    /// Exclude guides when looking up command help.
    #[arg(long = "exclude-guides")]
    pub exclude_guides: bool,

    /// Show all commands, including external commands (accepted for compat).
    #[arg(long = "no-external-commands")]
    pub no_external_commands: bool,

    /// Show all commands, excluding aliases (accepted for compat).
    #[arg(long = "no-aliases")]
    pub no_aliases: bool,

    /// Compact output for `help -a` (accepted for compat).
    #[arg(long = "no-verbose")]
    pub no_verbose: bool,

    /// Force info format.
    #[arg(short = 'i')]
    pub info: bool,

    /// Force man format.
    #[arg(short = 'm')]
    pub man: bool,

    /// Force web/html format.
    #[arg(short = 'w')]
    pub web: bool,

    /// Command to show help for.
    pub command: Option<String>,
}

fn alias_names_for_help() -> Vec<String> {
    let git_dir = std::env::var("GIT_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| {
            grit_lib::repo::Repository::discover(None)
                .ok()
                .map(|r| r.git_dir)
        });

    let Ok(config) = grit_lib::config::ConfigSet::load(git_dir.as_deref(), true) else {
        return Vec::new();
    };

    let mut names: Vec<String> = Vec::new();
    for entry in config.entries() {
        if !entry.key.starts_with("alias.") {
            continue;
        }
        let rest = &entry.key["alias.".len()..];
        if let Some(name) = rest.strip_suffix(".command") {
            if !name.is_empty() && !name.contains('.') {
                names.push(name.to_owned());
            }
            continue;
        }
        if let Some(name) = rest.strip_prefix('.') {
            if !name.is_empty() && !name.contains('.') {
                names.push(name.to_owned());
            }
            continue;
        }
        if !rest.is_empty() && !rest.contains('.') {
            names.push(rest.to_owned());
        }
    }

    names.sort();
    names.dedup();
    names
}

const GUIDE_PAGES: &[(&str, &str)] = &[
    ("core-tutorial", "gitcore-tutorial"),
    ("credentials", "gitcredentials"),
    ("cvs-migration", "gitcvs-migration"),
    ("diffcore", "gitdiffcore"),
    ("everyday", "giteveryday"),
    ("faq", "gitfaq"),
    ("glossary", "gitglossary"),
    ("namespaces", "gitnamespaces"),
    ("remote-helpers", "gitremote-helpers"),
    ("submodules", "gitsubmodules"),
    ("tutorial", "gittutorial"),
    ("tutorial-2", "gittutorial-2"),
    ("workflows", "gitworkflows"),
    ("revisions", "gitrevisions"),
];

fn guide_page_name(name: &str) -> Option<&'static str> {
    GUIDE_PAGES
        .iter()
        .find_map(|(guide, page)| (*guide == name).then_some(*page))
}

fn load_config_for_help() -> Option<ConfigSet> {
    let git_dir = std::env::var("GIT_DIR")
        .ok()
        .map(std::path::PathBuf::from)
        .or_else(|| Repository::discover(None).ok().map(|r| r.git_dir));
    ConfigSet::load(git_dir.as_deref(), true).ok()
}

fn usage_error() -> ! {
    eprintln!("usage: git help");
    std::process::exit(129);
}

fn should_use_web_viewer(args: &Args, config: Option<&ConfigSet>) -> bool {
    if args.info || args.man {
        return false;
    }
    if args.web {
        return true;
    }
    match config.and_then(|cfg| cfg.get("help.format")) {
        Some(fmt) => fmt.eq_ignore_ascii_case("html") || fmt.eq_ignore_ascii_case("web"),
        None => false,
    }
}

fn browser_command(config: &ConfigSet) -> Option<String> {
    let browser = config.get("help.browser").or_else(|| config.get("web.browser"))?;
    let browser_key = format!("browser.{browser}.cmd");
    config.get(&browser_key)
}

fn resolve_help_target(config: &ConfigSet, page: &str) -> Result<String> {
    let html_path = config
        .get("help.htmlpath")
        .unwrap_or_else(|| "Documentation".to_string());
    if html_path.contains("://") {
        return Ok(format!("{}/{}.html", html_path.trim_end_matches('/'), page));
    }

    let path = Path::new(&html_path).join(format!("{page}.html"));
    if !path.exists() {
        bail!("no HTML documentation found for '{page}'");
    }
    Ok(path.to_string_lossy().to_string())
}

fn open_in_browser(config: &ConfigSet, target: &str) -> Result<()> {
    let Some(cmd) = browser_command(config) else {
        bail!("no browser command configured");
    };
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{cmd} \"$1\""))
        .arg("git-help-browser")
        .arg(target)
        .status()?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn print_general_help(out: &mut dyn Write) -> Result<()> {
    writeln!(
        out,
        "usage: git [-v | --version] [-h | --help] [-C <path>] [-c <name>=<value>]"
    )?;
    writeln!(out)?;
    writeln!(
        out,
        "These are common Git commands used in various situations:"
    )?;
    writeln!(out)?;

    writeln!(out, "start a working area (see also: git help tutorial)")?;
    writeln!(out, "   clone      Clone a repository into a new directory")?;
    writeln!(
        out,
        "   init       Create an empty Git repository or reinitialize an existing one"
    )?;
    writeln!(out)?;

    writeln!(out, "work on the current change (see also: git help everyday)")?;
    writeln!(out, "   add        Add file contents to the index")?;
    writeln!(
        out,
        "   mv         Move or rename a file, a directory, or a symlink"
    )?;
    writeln!(out, "   restore    Restore working tree files")?;
    writeln!(
        out,
        "   rm         Remove files from the working tree and from the index"
    )?;
    writeln!(out)?;

    writeln!(out, "examine the history and state (see also: git help revisions)")?;
    writeln!(
        out,
        "   bisect     Use binary search to find the commit that introduced a bug"
    )?;
    writeln!(
        out,
        "   diff       Show changes between commits, commit and working tree, etc"
    )?;
    writeln!(out, "   grep       Print lines matching a pattern")?;
    writeln!(out, "   log        Show commit logs")?;
    writeln!(out, "   show       Show various types of objects")?;
    writeln!(out, "   status     Show the working tree status")?;
    writeln!(out)?;

    writeln!(out, "grow, mark and tweak your common history")?;
    writeln!(out, "   branch     List, create, or delete branches")?;
    writeln!(out, "   commit     Record changes to the repository")?;
    writeln!(out, "   merge      Join two or more development histories together")?;
    writeln!(out, "   rebase     Reapply commits on top of another base tip")?;
    writeln!(out, "   reset      Reset current HEAD to the specified state")?;
    writeln!(out, "   switch     Switch branches")?;
    writeln!(out, "   tag        Create, list, delete or verify a tag object")?;
    writeln!(out)?;

    writeln!(out, "collaborate (see also: git help workflows)")?;
    writeln!(out, "   fetch      Download objects and refs from another repository")?;
    writeln!(
        out,
        "   pull       Fetch from and integrate with another repository or a local branch"
    )?;
    writeln!(out, "   push       Update remote refs along with associated objects")?;
    writeln!(out)?;
    Ok(())
}

fn print_help_all(out: &mut dyn Write, include_aliases: bool) -> Result<()> {
    writeln!(
        out,
        "See 'git help <command>' to read about a specific subcommand"
    )?;
    writeln!(out)?;
    writeln!(out, "Main Porcelain Commands")?;
    writeln!(out, "   add        Add file contents to the index")?;
    writeln!(out, "   commit     Record changes to the repository")?;
    writeln!(out, "   status     Show the working tree status")?;
    writeln!(out)?;
    writeln!(out, "Ancillary Commands / Manipulators")?;
    writeln!(out, "   config     Get and set repository or global options")?;
    writeln!(out, "   remote     Manage set of tracked repositories")?;
    writeln!(out)?;
    writeln!(out, "Ancillary Commands / Interrogators")?;
    writeln!(out, "   blame      Show what revision and author last modified each line")?;
    writeln!(out, "   shortlog   Summarize 'git log' output")?;
    writeln!(out)?;
    writeln!(out, "Interacting with Others")?;
    writeln!(out, "   fetch      Download objects and refs from another repository")?;
    writeln!(out, "   pull       Fetch from and integrate with another repository")?;
    writeln!(out, "   push       Update remote refs along with associated objects")?;
    writeln!(out)?;
    writeln!(out, "Low-level Commands / Manipulators")?;
    writeln!(out, "   apply      Apply a patch to files and/or to the index")?;
    writeln!(out, "   update-ref Update stored ref safely")?;
    writeln!(out)?;
    writeln!(out, "Low-level Commands / Interrogators")?;
    writeln!(out, "   cat-file   Provide content or type and size information")?;
    writeln!(out, "   rev-parse  Pick out and massage parameters")?;
    writeln!(out)?;
    writeln!(out, "Low-level Commands / Syncing Repositories")?;
    writeln!(out, "   pack-refs  Pack heads and tags for efficient repository access")?;
    writeln!(out, "   unpack-objects  Unpack objects from a packed archive")?;
    writeln!(out)?;
    writeln!(out, "Low-level Commands / Internal Helpers")?;
    writeln!(out, "   merge-file  Run a three-way file merge")?;
    writeln!(out, "   verify-pack Validate packed Git archive files")?;
    writeln!(out)?;
    writeln!(out, "User-facing repository, command and file interfaces")?;
    writeln!(out, "   attributes  Defining attributes per path")?;
    writeln!(out, "   mailmap     Map author/committer names and email addresses")?;
    writeln!(out)?;
    writeln!(
        out,
        "Developer-facing file formats, protocols and other interfaces"
    )?;
    writeln!(out, "   protocol-capabilities  Protocol v0 and v1 capabilities")?;

    if include_aliases {
        let aliases = alias_names_for_help();
        if !aliases.is_empty() {
            writeln!(out)?;
            writeln!(out, "Command aliases")?;
            for alias in aliases {
                writeln!(out, "   {alias}")?;
            }
        }
    }
    Ok(())
}

fn print_guides(out: &mut dyn Write) -> Result<()> {
    writeln!(out, "The Git concept guides are:")?;
    writeln!(out)?;
    writeln!(out, "   everyday   A useful minimum set of commands for Everyday Git")?;
    writeln!(out, "   tutorial   A tutorial introduction to Git")?;
    writeln!(
        out,
        "   revisions  Specifying revisions and ranges for Git"
    )?;
    writeln!(out, "   workflows  An overview of recommended workflows with Git")?;
    Ok(())
}

fn print_user_interfaces(out: &mut dyn Write) -> Result<()> {
    writeln!(out, "User-facing repository, command and file interfaces")?;
    writeln!(out)?;
    writeln!(out, "   attributes   Defining attributes per path")?;
    writeln!(out, "   ignore       Specifies intentionally untracked files to ignore")?;
    writeln!(out, "   mailmap      Map author/committer names and email addresses")?;
    writeln!(out, "   modules      Defining submodule properties")?;
    Ok(())
}

/// Run `grit help`.
pub fn run(args: Args) -> Result<()> {
    let stdout = io::stdout();
    let mut out = stdout.lock();

    let list_mode_count = [
        args.all,
        args.guides,
        args.user_interfaces,
        args.config_list,
        args.config_for_completion,
        args.config_sections_for_completion,
    ]
    .into_iter()
    .filter(|v| *v)
    .count();

    if list_mode_count > 1 {
        usage_error();
    }
    if args.command.is_some() && list_mode_count > 0 {
        usage_error();
    }
    if (args.info || args.man || args.web) && list_mode_count > 0 {
        usage_error();
    }
    if (args.no_external_commands || args.no_aliases) && !args.all {
        usage_error();
    }

    if args.config_for_completion {
        print!("{CONFIG_VARS_FOR_COMPLETION}");
        return Ok(());
    }
    if args.config_list {
        print!("{CONFIG_VARS_ALL}");
        return Ok(());
    }
    if args.config_sections_for_completion {
        print!("{CONFIG_SECTIONS_FOR_COMPLETION}");
        return Ok(());
    }
    if args.guides {
        print_guides(&mut out)?;
        return Ok(());
    }
    if args.user_interfaces {
        print_user_interfaces(&mut out)?;
        return Ok(());
    }
    if args.all {
        print_help_all(&mut out, !args.no_aliases)?;
        return Ok(());
    }

    if let Some(cmd) = &args.command {
        if args.exclude_guides && guide_page_name(cmd).is_some() {
            std::process::exit(1);
        }

        let config = load_config_for_help();
        if should_use_web_viewer(&args, config.as_ref()) {
            if let Some(cfg) = config.as_ref() {
                let page = guide_page_name(cmd)
                    .map(str::to_string)
                    .unwrap_or_else(|| format!("git-{cmd}"));
                let target = resolve_help_target(cfg, &page)?;
                open_in_browser(cfg, &target)?;
                return Ok(());
            }
        }

        // Delegate to `git <command> --help` by re-execing ourselves.
        let exe = std::env::current_exe().unwrap_or_else(|_| "git".into());
        let status = std::process::Command::new(&exe).arg(cmd).arg("--help").status();
        match status {
            Ok(s) => {
                if s.success() {
                    return Ok(());
                }
                std::process::exit(s.code().unwrap_or(1));
            }
            Err(e) => bail!("failed to run help for '{cmd}': {e}"),
        }
    }

    print_general_help(&mut out)
}
