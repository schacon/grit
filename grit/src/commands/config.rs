//! `grit config` — read and modify Git configuration files.
//!
//! Supports both the legacy interface (`git config --get`, `git config key value`)
//! and the new subcommand interface (`git config get`, `git config set`).

use anyhow::{bail, Context, Result};
use clap::{Args as ClapArgs, Subcommand};
use grit_lib::config::{
    parse_bool, parse_color, parse_i64, parse_path, ConfigFile, ConfigScope, ConfigSet,
};
use grit_lib::objects::ObjectKind;
use grit_lib::repo::Repository;
use grit_lib::rev_parse::resolve_revision;
use std::path::{Path, PathBuf};

/// Arguments for `grit config`.
#[derive(Debug, ClapArgs)]
#[command(
    about = "Get and set repository or global options",
    after_help = "Use subcommands (get, set, unset, list) or legacy flags (--get, key value)."
)]
pub struct Args {
    #[command(subcommand)]
    pub subcommand: Option<ConfigSubcommand>,

    // ── File location flags ──
    /// Use the system-wide config file.
    #[arg(long, global = true)]
    pub system: bool,

    /// Use the global (per-user) config file.
    #[arg(long, global = true)]
    pub global: bool,

    /// Use the repository-local config file.
    #[arg(long, global = true)]
    pub local: bool,

    /// Use the per-worktree config file.
    #[arg(long, global = true)]
    pub worktree: bool,

    /// Use the given config file.
    #[arg(short = 'f', long = "file", global = true)]
    pub file: Option<PathBuf>,

    /// Read config from a blob object (e.g. HEAD:.gitmodules).
    #[arg(long = "blob", value_name = "BLOB_ISH")]
    pub blob: Option<String>,

    // ── Legacy action flags ──
    /// Get the value for a given key (legacy).
    #[arg(long = "get", value_name = "KEY")]
    pub get_key: Option<String>,

    /// Get all values for a multi-valued key (legacy).
    #[arg(long = "get-all", value_name = "KEY")]
    pub get_all_key: Option<String>,

    /// Get values matching a regex (legacy).
    #[arg(long = "get-regexp", value_name = "PATTERN")]
    pub get_regexp: Option<String>,

    /// Remove a key (legacy).
    #[arg(long = "unset", value_name = "KEY")]
    pub unset_key: Option<String>,

    /// Remove all occurrences of a key (legacy).
    #[arg(long = "unset-all", value_name = "KEY")]
    pub unset_all_key: Option<String>,

    /// List all config entries (legacy).
    #[arg(short = 'l', long = "list")]
    pub list: bool,

    /// Add a new line for a multi-valued key (legacy).
    #[arg(long = "add", value_name = "KEY")]
    pub add_key: Option<String>,

    /// Replace all matching values (legacy).
    #[arg(long = "replace-all")]
    pub replace_all: bool,

    /// Append an inline comment to the value.
    #[arg(long = "comment", global = true)]
    pub comment: Option<String>,

    /// Rename a section (legacy).
    #[arg(long = "rename-section")]
    pub rename_section: bool,

    /// Remove a section (legacy).
    #[arg(long = "remove-section")]
    pub remove_section: bool,

    // ── Type flags ──
    /// Ensure the value is a valid boolean and canonicalize.
    #[arg(long = "bool", global = true)]
    pub type_bool: bool,

    /// Ensure the value is a valid integer and canonicalize.
    #[arg(long = "int", global = true)]
    pub type_int: bool,

    /// Ensure the value is a valid bool-or-int and canonicalize.
    #[arg(long = "bool-or-int", global = true)]
    pub type_bool_or_int: bool,

    /// Expand `~/` in the value.
    #[arg(long = "path", global = true)]
    pub type_path: bool,

    /// Type selector (alternative to individual flags).
    #[arg(long = "type", value_name = "TYPE", global = true)]
    pub type_name: Option<String>,

    // ── Display flags ──
    /// Show origin file and scope for each entry.
    #[arg(long = "show-origin")]
    pub show_origin: bool,

    /// Show scope for each entry.
    #[arg(long = "show-scope")]
    pub show_scope: bool,

    /// Use NUL as delimiter.
    #[arg(short = 'z')]
    pub null_terminated: bool,

    /// Show key names for --get-regexp.
    #[arg(long = "name-only")]
    pub name_only: bool,

    /// Includes support.
    #[arg(long = "includes")]
    pub includes: bool,

    /// Do not honour include directives.
    #[arg(long = "no-includes")]
    pub no_includes: bool,

    /// Default value if key is not found (legacy --get/--get-all).
    #[arg(long = "default", value_name = "VALUE", global = true)]
    pub default_value: Option<String>,

    /// Only match exact values (instead of treating value as regex).
    #[arg(long = "fixed-value", global = true)]
    pub fixed_value: bool,

    // ── URL match flags ──
    /// Get the best-matching value for the given URL.
    #[arg(long = "get-urlmatch", value_name = "KEY", num_args = 1)]
    pub get_urlmatch_key: Option<String>,

    /// Get the color setting (legacy): returns ANSI code for the color, with default.
    #[arg(long = "get-color", value_name = "KEY", num_args = 1)]
    pub get_color_key: Option<String>,

    // ── Positional args for legacy set (`git config key value`) ──
    /// Positional arguments (key, value, value-pattern for legacy mode).
    #[arg(trailing_var_arg = true)]
    pub positional: Vec<String>,
}

/// Modern subcommand interface for `grit config`.
#[derive(Debug, Subcommand)]
pub enum ConfigSubcommand {
    /// Get the value for a key.
    Get(GetArgs),
    /// Set a key to a value.
    Set(SetArgs),
    /// Unset (remove) a key.
    Unset(UnsetArgs),
    /// List all config entries.
    List(ListArgs),
    /// Rename a section.
    #[command(name = "rename-section")]
    RenameSection(RenameSectionArgs),
    /// Remove a section.
    #[command(name = "remove-section")]
    RemoveSection(RemoveSectionArgs),
    /// Open the config file in an editor.
    Edit(EditArgs),
}

/// Arguments for `grit config get`.
#[derive(Debug, ClapArgs)]
pub struct GetArgs {
    /// The configuration key.
    pub key: String,

    /// Get all values (multi-valued key).
    #[arg(long)]
    pub all: bool,

    /// Treat key as a regex.
    #[arg(long)]
    pub regexp: bool,

    /// Show key names alongside values.
    #[arg(long = "show-names")]
    pub show_names: bool,

    /// Default value if key is missing.
    #[arg(long)]
    pub default: Option<String>,

    /// Match config against a URL.
    #[arg(long = "url")]
    pub url: Option<String>,

    /// Show origin file and scope for each entry.
    #[arg(long = "show-origin")]
    pub show_origin: bool,

    /// Show scope for each entry.
    #[arg(long = "show-scope")]
    pub show_scope: bool,
}

/// Arguments for `grit config set`.
#[derive(Debug, ClapArgs)]
pub struct SetArgs {
    /// The configuration key.
    pub key: String,
    /// The value to set.
    pub value: String,

    /// Replace all matching values.
    #[arg(long)]
    pub all: bool,

    /// Append a new line for a multi-valued key.
    #[arg(long)]
    pub append: bool,
}

/// Arguments for `grit config unset`.
#[derive(Debug, ClapArgs)]
pub struct UnsetArgs {
    /// The configuration key.
    pub key: String,

    /// Remove all occurrences.
    #[arg(long)]
    pub all: bool,
}

/// Arguments for `grit config list`.
#[derive(Debug, ClapArgs)]
pub struct ListArgs {
    /// Show only names, not values.
    #[arg(long = "name-only")]
    pub name_only: bool,

    /// Show config file path.
    #[arg(long = "show-origin")]
    pub show_origin: bool,

    /// Show config scope.
    #[arg(long = "show-scope")]
    pub show_scope: bool,
}

/// Arguments for `grit config rename-section`.
#[derive(Debug, ClapArgs)]
pub struct RenameSectionArgs {
    /// Old section name.
    pub old_name: String,
    /// New section name.
    pub new_name: String,
}

/// Arguments for `grit config remove-section`.
#[derive(Debug, ClapArgs)]
pub struct RemoveSectionArgs {
    /// Section name to remove.
    pub name: String,
}

/// Arguments for `grit config edit`.
#[derive(Debug, ClapArgs)]
pub struct EditArgs {}

// ── Entrypoint ──────────────────────────────────────────────────────

/// Run the `config` command.
pub fn run(args: Args) -> Result<()> {
    // If --blob is given, read config from the blob and handle read-only ops
    if let Some(ref blob_spec) = args.blob {
        // --blob is incompatible with file-scope flags
        if args.system || args.global || args.local || args.worktree || args.file.is_some() {
            bail!("--blob and file-location options (--system, --global, --local, --worktree, --file) are incompatible");
        }
        return cmd_blob(&args, blob_spec);
    }

    // Resolve which file to operate on
    let git_dir = resolve_git_dir();
    let (scope, file_path) = resolve_config_file(&args, git_dir.as_deref())?;

    // Handle subcommands first
    if let Some(ref sub) = args.subcommand {
        return match sub {
            ConfigSubcommand::Get(get_args) => cmd_get(&args, get_args, git_dir.as_deref(), None),
            ConfigSubcommand::Set(set_args) => cmd_set(&args, set_args, scope, &file_path, None),
            ConfigSubcommand::Unset(unset_args) => {
                cmd_unset(&args, unset_args, scope, &file_path, None)
            }
            ConfigSubcommand::List(list_args) => {
                // Merge list-level flags into top-level args
                let mut merged = Args {
                    name_only: args.name_only || list_args.name_only,
                    show_origin: args.show_origin || list_args.show_origin,
                    show_scope: args.show_scope || list_args.show_scope,
                    ..args
                };
                merged.subcommand = None; // avoid borrow issues
                cmd_list(&merged, git_dir.as_deref())
            }
            ConfigSubcommand::RenameSection(rs) => {
                cmd_rename_section(scope, &file_path, &rs.old_name, &rs.new_name)
            }
            ConfigSubcommand::RemoveSection(rs) => cmd_remove_section(scope, &file_path, &rs.name),
            ConfigSubcommand::Edit(_) => cmd_edit(&file_path),
        };
    }

    // Legacy interface
    if args.list {
        return cmd_list(&args, git_dir.as_deref());
    }

    if let Some(ref key) = args.get_key {
        let value_pattern = args.positional.first().map(|s| s.as_str());
        let get_args = GetArgs {
            key: key.clone(),
            all: false,
            regexp: false,
            show_names: false,
            default: args.default_value.clone(),
            url: None,
            show_origin: false,
            show_scope: false,
        };
        return cmd_get(&args, &get_args, git_dir.as_deref(), value_pattern);
    }

    if let Some(ref key) = args.get_all_key {
        let value_pattern = args.positional.first().map(|s| s.as_str());
        let get_args = GetArgs {
            key: key.clone(),
            all: true,
            regexp: false,
            show_names: false,
            default: args.default_value.clone(),
            url: None,
            show_origin: false,
            show_scope: false,
        };
        return cmd_get(&args, &get_args, git_dir.as_deref(), value_pattern);
    }

    if let Some(ref pattern) = args.get_regexp {
        let get_args = GetArgs {
            key: pattern.clone(),
            all: true,
            regexp: true,
            show_names: true,
            default: args.default_value.clone(),
            url: None,
            show_origin: false,
            show_scope: false,
        };
        return cmd_get(&args, &get_args, git_dir.as_deref(), None);
    }

    if let Some(ref key) = args.get_urlmatch_key {
        if args.positional.is_empty() {
            bail!("usage: git config --get-urlmatch <key> <URL>");
        }
        return cmd_get_urlmatch(&args, key, &args.positional[0], git_dir.as_deref());
    }

    if let Some(ref key) = args.get_color_key {
        let default_color = args.positional.first().map(|s| s.as_str()).unwrap_or("");
        return cmd_get_color(key, default_color, git_dir.as_deref());
    }

    // Validate --default is only used with get operations
    if args.default_value.is_some() {
        let is_get_op = args.get_key.is_some()
            || args.get_all_key.is_some()
            || args.get_regexp.is_some()
            || args.get_urlmatch_key.is_some();
        if !is_get_op {
            let is_positional_get = args.positional.len() <= 1
                && args.unset_key.is_none()
                && args.unset_all_key.is_none()
                && args.add_key.is_none()
                && !args.remove_section
                && !args.rename_section
                && !args.list;
            if !is_positional_get {
                eprintln!("error: --default is only applicable to --get, --get-all, --get-regexp, and --get-urlmatch");
                std::process::exit(129);
            }
        }
    }

    if let Some(ref key) = args.unset_key {
        let unset_args = UnsetArgs {
            key: key.clone(),
            all: false,
        };
        let value_pattern = args.positional.first().map(|s| s.as_str());
        return cmd_unset(&args, &unset_args, scope, &file_path, value_pattern);
    }

    if let Some(ref key) = args.unset_all_key {
        let unset_args = UnsetArgs {
            key: key.clone(),
            all: true,
        };
        let value_pattern = args.positional.first().map(|s| s.as_str());
        return cmd_unset(&args, &unset_args, scope, &file_path, value_pattern);
    }

    if let Some(ref key) = args.add_key {
        if args.positional.is_empty() {
            bail!("missing value for --add");
        }
        return cmd_add(&args, key, &args.positional[0], scope, &file_path);
    }

    if args.remove_section {
        if args.positional.is_empty() {
            bail!("missing section name");
        }
        return cmd_remove_section(scope, &file_path, &args.positional[0]);
    }

    if args.rename_section {
        if args.positional.len() < 2 {
            bail!("missing old-name and/or new-name");
        }
        return cmd_rename_section(scope, &file_path, &args.positional[0], &args.positional[1]);
    }

    // Legacy set: `git config key value`
    match args.positional.len() {
        0 => {
            // No args, no flags → show usage
            bail!("usage: grit config [<options>]");
        }
        1 => {
            if args.replace_all {
                bail!("error: wrong number of arguments, should be 2");
            }
            // Legacy get: `git config key`
            let get_args = GetArgs {
                key: args.positional[0].clone(),
                all: false,
                regexp: false,
                show_names: false,
                default: args.default_value.clone(),
                url: None,
                show_origin: false,
                show_scope: false,
            };
            cmd_get(&args, &get_args, git_dir.as_deref(), None)
        }
        2 => {
            // Legacy set: `git config key value`
            // or `git config --replace-all key value`
            let set_args = SetArgs {
                key: args.positional[0].clone(),
                value: args.positional[1].clone(),
                all: args.replace_all,
            };
            cmd_set(&args, &set_args, scope, &file_path, None)
        }
        3 => {
            if args.replace_all {
                // `git config --replace-all key value value-pattern`
                let set_args = SetArgs {
                    key: args.positional[0].clone(),
                    value: args.positional[1].clone(),
                    all: true,
                };
                cmd_set(
                    &args,
                    &set_args,
                    scope,
                    &file_path,
                    Some(&args.positional[2]),
                )
            } else {
                // `git config key value value-pattern` (legacy with value-pattern)
                let set_args = SetArgs {
                    key: args.positional[0].clone(),
                    value: args.positional[1].clone(),
                    all: false,
                };
                cmd_set(
                    &args,
                    &set_args,
                    scope,
                    &file_path,
                    Some(&args.positional[2]),
                )
            }
        }
        _ => bail!("too many arguments"),
    }
}

// ── Subcommand implementations ──────────────────────────────────────

fn cmd_get(
    args: &Args,
    get_args: &GetArgs,
    git_dir: Option<&Path>,
    value_pattern: Option<&str>,
) -> Result<()> {
    let config = load_config(args, git_dir)?;
    let terminator = if args.null_terminated { '\0' } else { '\n' };

    // Handle --url for URL matching (subcommand interface)
    if let Some(ref url) = get_args.url {
        let (section, variable) = match get_args.key.find('.') {
            Some(i) => (&get_args.key[..i], &get_args.key[i + 1..]),
            None => bail!("key does not contain a section: '{}'", get_args.key),
        };
        let entries =
            grit_lib::config::get_urlmatch_entries(config.entries(), section, variable, url);
        if entries.is_empty() {
            if let Some(ref default) = get_args.default {
                let val = format_typed_value(args, default)?;
                print!("{val}{terminator}");
                return Ok(());
            }
            std::process::exit(1);
        }
        let entry = entries.last().unwrap();
        let val = entry.value.as_deref().unwrap_or("true");
        let val = format_typed_value(args, val)?;
        print!("{val}{terminator}");
        return Ok(());
    }

    if get_args.regexp {
        let matches = config
            .get_regexp(&get_args.key)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        if matches.is_empty() {
            std::process::exit(1);
        }
        for entry in matches {
            let _has_type_flag = args.type_bool
                || args.type_int
                || args.type_bool_or_int
                || args.type_path
                || args.type_name.is_some();
            let _is_bare = entry.value.is_none();
            let val = entry.value.as_deref().unwrap_or("true");
            let val = format_typed_value(args, val)?;
            if args.name_only {
                print!("{}{}", entry.key, terminator);
            } else if get_args.show_names {
                print!("{} {}{}", entry.key, val, terminator);
            } else {
                print!("{}{}", val, terminator);
            }
        }
        return Ok(());
    }

    if get_args.all {
        let mut values = config.get_all(&get_args.key);
        if let Some(pattern) = value_pattern {
            filter_values_by_pattern(&mut values, pattern, args.fixed_value)?;
        }
        if values.is_empty() {
            if let Some(ref default) = get_args.default {
                let val = format_typed_value(args, default)?;
                print!("{val}{terminator}");
                return Ok(());
            }
            std::process::exit(1);
        }
        for val in values {
            let val = format_typed_value(args, &val)?;
            print!("{val}{terminator}");
        }
        return Ok(());
    }

    if let Some(pattern) = value_pattern {
        // --get with value-regex: get all values, filter, return last match
        let mut values = config.get_all(&get_args.key);
        filter_values_by_pattern(&mut values, pattern, args.fixed_value)?;
        if let Some(val) = values.last() {
            let val = format_typed_value(args, val)?;
            print!("{val}{terminator}");
            return Ok(());
        }
        if let Some(ref default) = get_args.default {
            let d = format_typed_value(args, default)?;
            print!("{d}{terminator}");
            return Ok(());
        }
        std::process::exit(1);
    }

    match config.get(&get_args.key) {
        Some(val) => {
            let val = format_typed_value(args, &val)?;
            print!("{val}{terminator}");
            Ok(())
        }
        None => {
            if let Some(ref default) = get_args.default {
                let val = format_typed_value(args, default)?;
                print!("{val}{terminator}");
                return Ok(());
            }
            std::process::exit(1);
        }
    }
}

fn cmd_set(
    args: &Args,
    set_args: &SetArgs,
    scope: ConfigScope,
    file_path: &Path,
    value_pattern: Option<&str>,
) -> Result<()> {
    // Validate --comment: must not contain LF
    if let Some(ref c) = args.comment {
        if c.contains('\n') {
            bail!("invalid comment: must not contain newline");
        }
    }

    // Canonicalize the value if a type flag is given
    let value = canonicalize_value_for_set(args, &set_args.value)?;
    let comment = args.comment.as_deref();

    let mut config = match ConfigFile::from_path(file_path, scope).context("reading config file")? {
        Some(cfg) => cfg,
        None => ConfigFile::parse(file_path, "", scope)?,
    };

    if set_args.append {
        config.add_value(&set_args.key, &value)?;
    } else if set_args.all {
        config.replace_all_with_comment(&set_args.key, &value, value_pattern, comment)?;
    } else if let Some(pattern) = value_pattern {
        config.replace_all_with_comment(&set_args.key, &value, Some(pattern), comment)?;
    } else {
        config.set_with_comment(&set_args.key, &value, comment)?;
    }
    config.write().context("writing config file")?;
    Ok(())
}

fn cmd_unset(
    _args: &Args,
    unset_args: &UnsetArgs,
    scope: ConfigScope,
    file_path: &Path,
    value_pattern: Option<&str>,
) -> Result<()> {
    let mut config = ConfigFile::from_path(file_path, scope).context("reading config file")?;

    match config {
        Some(ref mut cfg) => {
            if unset_args.all {
                let removed = cfg.unset_matching(&unset_args.key, value_pattern)?;
                if removed == 0 {
                    std::process::exit(5);
                }
            } else if let Some(pattern) = value_pattern {
                // --unset with value-pattern: remove only matching values
                let removed = cfg.unset_matching(&unset_args.key, Some(pattern))?;
                if removed == 0 {
                    std::process::exit(5);
                }
            } else {
                // --unset (single): fail if multiple values exist
                let count = cfg.count(&unset_args.key)?;
                if count == 0 {
                    std::process::exit(5);
                }
                if count > 1 {
                    eprintln!("warning: {}: has multiple values", unset_args.key);
                    std::process::exit(5);
                }
                let removed = cfg.unset_matching(&unset_args.key, None)?;
                if removed == 0 {
                    std::process::exit(5);
                }
            }
            cfg.write().context("writing config file")?;
        }
        None => std::process::exit(5),
    }
    Ok(())
}

fn cmd_list(args: &Args, git_dir: Option<&Path>) -> Result<()> {
    let config = load_config(args, git_dir)?;
    let terminator = if args.null_terminated { '\0' } else { '\n' };
    let cwd = std::env::current_dir().ok();

    for entry in config.entries() {
        let mut prefix = String::new();
        if args.show_scope {
            prefix.push_str(&format!("{}\t", entry.scope));
        }
        if args.show_origin {
            if let Some(ref file) = entry.file {
                // Use relative path if possible (matches git behavior)
                let display_path = if let Some(ref cwd) = cwd {
                    if let Ok(rel) = file.strip_prefix(cwd) {
                        rel.display().to_string()
                    } else {
                        file.display().to_string()
                    }
                } else {
                    file.display().to_string()
                };
                prefix.push_str(&format!("file:{}\t", display_path));
            }
        }
        let val = entry.value.as_deref().unwrap_or("true");
        if args.name_only {
            print!("{}{}{}", prefix, entry.key, terminator);
        } else if args.null_terminated {
            // Git uses key=value\0 format with -z for --list
            print!("{}{}={}{}", prefix, entry.key, val, terminator);
        } else {
            print!("{}{}={}{}", prefix, entry.key, val, terminator);
        }
    }
    Ok(())
}

fn cmd_remove_section(scope: ConfigScope, file_path: &Path, name: &str) -> Result<()> {
    let mut config = ConfigFile::from_path(file_path, scope).context("reading config file")?;

    match config {
        Some(ref mut cfg) => {
            if !cfg.remove_section(name)? {
                bail!("no such section: {name}");
            }
            cfg.write().context("writing config file")?;
        }
        None => bail!("config file not found: {}", file_path.display()),
    }
    Ok(())
}

fn cmd_rename_section(
    scope: ConfigScope,
    file_path: &Path,
    old_name: &str,
    new_name: &str,
) -> Result<()> {
    let mut config = ConfigFile::from_path(file_path, scope).context("reading config file")?;

    match config {
        Some(ref mut cfg) => {
            if !cfg.rename_section(old_name, new_name)? {
                bail!("no such section: {old_name}");
            }
            cfg.write().context("writing config file")?;
        }
        None => bail!("config file not found: {}", file_path.display()),
    }
    Ok(())
}

fn cmd_add(
    _args: &Args,
    key: &str,
    value: &str,
    scope: ConfigScope,
    file_path: &Path,
) -> Result<()> {
    let mut config = match ConfigFile::from_path(file_path, scope).context("reading config file")? {
        Some(cfg) => cfg,
        None => ConfigFile::parse(file_path, "", scope)?,
    };
    config.add_value(key, value)?;
    config.write().context("writing config file")?;
    Ok(())
}

fn cmd_edit(file_path: &Path) -> Result<()> {
    // Resolve editor: GIT_EDITOR env → core.editor config → VISUAL env → EDITOR env → vi
    let git_dir = resolve_git_dir();
    let config = ConfigSet::load(git_dir.as_deref(), true).unwrap_or_default();

    let editor = std::env::var("GIT_EDITOR")
        .ok()
        .or_else(|| config.get("core.editor"))
        .or_else(|| std::env::var("VISUAL").ok())
        .or_else(|| std::env::var("EDITOR").ok())
        .unwrap_or_else(|| "vi".to_owned());

    // Use shell to handle editors that include arguments/redirections
    // (matches Git's launch_editor behavior)
    let file_str = file_path.display().to_string();
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{} \"$@\"", editor))
        .arg("--")
        .arg(&file_str)
        .status()
        .with_context(|| format!("failed to launch editor '{editor}'"))?;

    if !status.success() {
        bail!("editor exited with status {}", status);
    }
    Ok(())
}

/// Handle `--blob=<blob-ish>` — read config from a blob object (read-only).
/// Handle `--get-urlmatch <key> <URL>`.
fn cmd_get_urlmatch(args: &Args, key: &str, url: &str, git_dir: Option<&Path>) -> Result<()> {
    let config = load_config(args, git_dir)?;
    let terminator = if args.null_terminated { '\0' } else { '\n' };

    if let Some(dot) = key.find('.') {
        let section = &key[..dot];
        let variable = &key[dot + 1..];
        let entries =
            grit_lib::config::get_urlmatch_entries(config.entries(), section, variable, url);
        if entries.is_empty() {
            std::process::exit(1);
        }
        let entry = entries.last().unwrap();
        let val = entry.value.as_deref().unwrap_or("true");
        let val = format_typed_value(args, val)?;
        print!("{val}{terminator}");
    } else {
        // Section-only: return all variables from that section matching the URL
        let entries = grit_lib::config::get_urlmatch_all_in_section(config.entries(), key, url);
        if entries.is_empty() {
            std::process::exit(1);
        }
        for (var_key, val, scope) in &entries {
            let val = format_typed_value(args, val)?;
            let prefix = if args.show_scope {
                format!("{}\t", scope)
            } else {
                String::new()
            };
            print!("{prefix}{var_key} {val}{terminator}");
        }
    }
    Ok(())
}

/// Handle `--get-color <key> [<default>]`.
fn cmd_get_color(key: &str, default_color: &str, git_dir: Option<&Path>) -> Result<()> {
    let git_dir_resolved = git_dir.map(|p| p.to_path_buf());
    let config = ConfigSet::load(git_dir_resolved.as_deref(), true).unwrap_or_default();

    let color_str = if !key.is_empty() {
        config.get(key).unwrap_or_else(|| default_color.to_owned())
    } else {
        default_color.to_owned()
    };

    if color_str.is_empty() {
        return Ok(());
    }

    let ansi = parse_color(&color_str).map_err(|e| anyhow::anyhow!("{}", e))?;
    print!("{ansi}");
    Ok(())
}

fn cmd_blob(args: &Args, blob_spec: &str) -> Result<()> {
    let repo = Repository::discover(None).context("not a git repository")?;
    let oid = resolve_revision(&repo, blob_spec)
        .map_err(|_| anyhow::anyhow!("unable to resolve spec '{}' to a blob", blob_spec))?;
    let obj = repo
        .odb
        .read(&oid)
        .map_err(|_| anyhow::anyhow!("unable to read object {}", oid))?;
    if obj.kind != ObjectKind::Blob {
        bail!(
            "object {} is a {}, not a blob",
            oid,
            match obj.kind {
                ObjectKind::Tree => "tree",
                ObjectKind::Commit => "commit",
                ObjectKind::Tag => "tag",
                _ => "unknown",
            }
        );
    }
    let content = String::from_utf8(obj.data).context("blob is not valid UTF-8")?;
    let blob_path = std::path::Path::new("<blob>");
    let config_file = ConfigFile::parse(blob_path, &content, ConfigScope::Command)?;
    let mut config = ConfigSet::new();
    config.merge(&config_file);

    let terminator = if args.null_terminated { '\0' } else { '\n' };

    // --list
    if args.list {
        for entry in config.entries() {
            let val = entry.value.as_deref().unwrap_or("true");
            if args.name_only {
                print!("{}{}", entry.key, terminator);
            } else {
                print!("{}={}{}", entry.key, val, terminator);
            }
        }
        return Ok(());
    }

    // --get-regexp
    if let Some(ref pattern) = args.get_regexp {
        let matches = config
            .get_regexp(pattern)
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        if matches.is_empty() {
            std::process::exit(1);
        }
        for entry in matches {
            let val = entry.value.as_deref().unwrap_or("true");
            let val = format_typed_value(args, val)?;
            if args.name_only {
                print!("{}{}", entry.key, terminator);
            } else {
                print!("{} {}{}", entry.key, val, terminator);
            }
        }
        return Ok(());
    }

    // --get
    if let Some(ref key) = args.get_key {
        match config.get(key) {
            Some(val) => {
                let val = format_typed_value(args, &val)?;
                print!("{val}{terminator}");
                return Ok(());
            }
            None => std::process::exit(1),
        }
    }

    // --get-all
    if let Some(ref key) = args.get_all_key {
        let values = config.get_all(key);
        if values.is_empty() {
            std::process::exit(1);
        }
        for val in values {
            let val = format_typed_value(args, &val)?;
            print!("{val}{terminator}");
        }
        return Ok(());
    }

    // Positional: `git config --blob=X key`
    if args.positional.len() == 1 {
        match config.get(&args.positional[0]) {
            Some(val) => {
                let val = format_typed_value(args, &val)?;
                print!("{val}{terminator}");
                return Ok(());
            }
            None => std::process::exit(1),
        }
    }

    if args.positional.is_empty() && args.subcommand.is_none() {
        bail!("--blob requires a key or --list");
    }

    // Handle subcommands (get/list) with blob
    if let Some(ref sub) = args.subcommand {
        match sub {
            ConfigSubcommand::Get(get_args) => {
                if get_args.regexp {
                    let matches = config
                        .get_regexp(&get_args.key)
                        .map_err(|e| anyhow::anyhow!("{}", e))?;
                    if matches.is_empty() {
                        std::process::exit(1);
                    }
                    for entry in matches {
                        let val = entry.value.as_deref().unwrap_or("true");
                        let val = format_typed_value(args, val)?;
                        if get_args.show_names {
                            print!("{} {}{}", entry.key, val, terminator);
                        } else {
                            print!("{}{}", val, terminator);
                        }
                    }
                    return Ok(());
                }
                if get_args.all {
                    let values = config.get_all(&get_args.key);
                    if values.is_empty() {
                        std::process::exit(1);
                    }
                    for val in values {
                        let val = format_typed_value(args, &val)?;
                        print!("{val}{terminator}");
                    }
                    return Ok(());
                }
                match config.get(&get_args.key) {
                    Some(val) => {
                        let val = format_typed_value(args, &val)?;
                        print!("{val}{terminator}");
                        Ok(())
                    }
                    None => std::process::exit(1),
                }
            }
            ConfigSubcommand::List(_) => {
                for entry in config.entries() {
                    let val = entry.value.as_deref().unwrap_or("true");
                    if args.name_only {
                        print!("{}{}", entry.key, terminator);
                    } else {
                        print!("{}={}{}", entry.key, val, terminator);
                    }
                }
                Ok(())
            }
            _ => bail!("--blob is read-only; cannot set/unset/edit"),
        }
    } else {
        bail!("--blob requires a key or --list");
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Filter a list of values by a value-pattern.
///
/// If `fixed_value` is true, the pattern is treated as a literal string.
/// Otherwise it is treated as a regex. A `!` prefix inverts the match.
fn filter_values_by_pattern(
    values: &mut Vec<String>,
    pattern: &str,
    fixed_value: bool,
) -> Result<()> {
    if fixed_value {
        values.retain(|v| v == pattern);
    } else {
        let (negated, pat) = if let Some(rest) = pattern.strip_prefix('!') {
            (true, rest)
        } else {
            (false, pattern)
        };
        let re = regex::Regex::new(pat)
            .with_context(|| format!("invalid value-pattern regex: {pat}"))?;
        values.retain(|v| {
            let matched = re.is_match(v);
            if negated {
                !matched
            } else {
                matched
            }
        });
    }
    Ok(())
}

/// Resolve the git directory (best-effort; returns None outside a repo).
fn resolve_git_dir() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("GIT_DIR") {
        return Some(PathBuf::from(dir));
    }
    // Walk up from cwd looking for .git
    let cwd = std::env::current_dir().ok()?;
    let mut cur = cwd.as_path();
    loop {
        let dot_git = cur.join(".git");
        if dot_git.is_dir() {
            return Some(dot_git);
        }
        if dot_git.is_file() {
            // gitfile
            if let Ok(content) = std::fs::read_to_string(&dot_git) {
                for line in content.lines() {
                    if let Some(rest) = line.strip_prefix("gitdir:") {
                        let path = rest.trim();
                        let resolved = if Path::new(path).is_absolute() {
                            PathBuf::from(path)
                        } else {
                            cur.join(path)
                        };
                        return Some(resolved);
                    }
                }
            }
        }
        // Check if cur itself is a bare repo
        if cur.join("objects").is_dir() && cur.join("HEAD").is_file() {
            return Some(cur.to_path_buf());
        }
        cur = cur.parent()?;
    }
}

/// Determine which config file to write to based on flags.
fn resolve_config_file(args: &Args, git_dir: Option<&Path>) -> Result<(ConfigScope, PathBuf)> {
    if let Some(ref path) = args.file {
        return Ok((ConfigScope::Local, path.clone()));
    }
    if args.system {
        let path = std::env::var("GIT_CONFIG_SYSTEM")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/etc/gitconfig"));
        return Ok((ConfigScope::System, path));
    }
    if args.global {
        let path = global_config_path()
            .ok_or_else(|| anyhow::anyhow!("cannot determine global config path"))?;
        return Ok((ConfigScope::Global, path));
    }
    if args.worktree {
        let gd = git_dir.ok_or_else(|| anyhow::anyhow!("not in a git repository"))?;
        return Ok((ConfigScope::Worktree, gd.join("config.worktree")));
    }
    // Default: local
    if let Some(gd) = git_dir {
        Ok((ConfigScope::Local, gd.join("config")))
    } else {
        // Outside repo, default to global for read operations
        let path = global_config_path().unwrap_or_else(|| PathBuf::from("/etc/gitconfig"));
        Ok((ConfigScope::Global, path))
    }
}

/// Load the config set, respecting file-scope flags.
fn load_config(args: &Args, git_dir: Option<&Path>) -> Result<ConfigSet> {
    // If a specific file is requested, only read that file
    if let Some(ref path) = args.file {
        let mut set = ConfigSet::new();
        if let Some(f) = ConfigFile::from_path(path, ConfigScope::Local)? {
            set.merge(&f);
        }
        return Ok(set);
    }

    if args.system {
        let mut set = ConfigSet::new();
        let system_path = std::env::var("GIT_CONFIG_SYSTEM")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("/etc/gitconfig"));
        if let Some(f) = ConfigFile::from_path(&system_path, ConfigScope::System)? {
            set.merge(&f);
        }
        return Ok(set);
    }

    if args.global {
        let mut set = ConfigSet::new();
        if let Some(path) = global_config_path() {
            if let Some(f) = ConfigFile::from_path(&path, ConfigScope::Global)? {
                set.merge(&f);
            }
        }
        return Ok(set);
    }

    if args.local {
        let mut set = ConfigSet::new();
        if let Some(gd) = git_dir {
            if let Some(f) = ConfigFile::from_path(&gd.join("config"), ConfigScope::Local)? {
                set.merge(&f);
            }
        }
        return Ok(set);
    }

    // Default: full cascade
    Ok(ConfigSet::load(git_dir, true)?)
}

/// Get the path for the global config file.
fn global_config_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("GIT_CONFIG_GLOBAL") {
        return Some(PathBuf::from(p));
    }
    std::env::var("HOME")
        .ok()
        .map(|h| PathBuf::from(h).join(".gitconfig"))
}

/// Canonicalize a value for writing based on type flags.
///
/// When `--bool` is used, the value is validated and written as "true"/"false".
/// When `--int` is used, the value is validated and written as a plain integer.
/// When `--bool-or-int` is used, booleans are stored as "true"/"false" and
/// integers as plain numbers.
fn canonicalize_value_for_set(args: &Args, val: &str) -> Result<String> {
    let type_name = args.type_name.as_deref();

    if args.type_bool || type_name == Some("bool") {
        match parse_bool(val) {
            Ok(b) => return Ok(if b { "true" } else { "false" }.to_owned()),
            Err(e) => bail!("{}", e),
        }
    }

    if args.type_int || type_name == Some("int") {
        match parse_i64(val) {
            Ok(n) => return Ok(n.to_string()),
            Err(e) => bail!("{}", e),
        }
    }

    if args.type_bool_or_int || type_name == Some("bool-or-int") {
        // Try named booleans first (not numbers — those go to int)
        match val.to_lowercase().as_str() {
            "true" | "yes" | "on" => return Ok("true".to_owned()),
            "false" | "no" | "off" => return Ok("false".to_owned()),
            _ => {}
        }
        // Then as integer
        if let Ok(n) = parse_i64(val) {
            return Ok(n.to_string());
        }
        bail!("bad bool-or-int config value '{}'", val);
    }

    if type_name == Some("color") {
        match parse_color(val) {
            Ok(_) => return Ok(val.to_owned()),
            Err(e) => bail!("{}", e),
        }
    }

    Ok(val.to_owned())
}

/// Format a value according to the type flags.
fn format_typed_value(args: &Args, val: &str) -> Result<String> {
    let type_name = args.type_name.as_deref();

    if args.type_bool || type_name == Some("bool") {
        match parse_bool(val) {
            Ok(b) => {
                return Ok(if b {
                    "true".to_owned()
                } else {
                    "false".to_owned()
                })
            }
            Err(e) => bail!("{}", e),
        }
    }

    if args.type_int || type_name == Some("int") {
        match parse_i64(val) {
            Ok(n) => return Ok(n.to_string()),
            Err(e) => bail!("{}", e),
        }
    }

    if args.type_path || type_name == Some("path") {
        return Ok(parse_path(val));
    }

    if args.type_bool_or_int || type_name == Some("bool-or-int") {
        // Try as named bool first
        match val.to_lowercase().as_str() {
            "true" | "yes" | "on" | "" => return Ok("true".to_owned()),
            "false" | "no" | "off" => return Ok("false".to_owned()),
            _ => {}
        }
        // Then as integer
        match parse_i64(val) {
            Ok(n) => return Ok(n.to_string()),
            Err(e) => bail!("{}", e),
        }
    }

    if type_name == Some("color") {
        match parse_color(val) {
            Ok(ansi) => return Ok(ansi),
            Err(e) => bail!("{}", e),
        }
    }

    Ok(val.to_owned())
}
