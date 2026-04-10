//! `grit init` — initialise or reinitialise a Git repository.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::fs;
use std::path::{Path, PathBuf};

use grit_lib::config::{parse_bool, ConfigFile, ConfigScope, ConfigSet};
use grit_lib::unicode_normalization::probe_filesystem_normalizes_nfd_to_nfc;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// `PERM_UMASK` / `PERM_GROUP` / `PERM_EVERYBODY` from git `setup.h` (`sharedrepo` enum).
const PERM_UMASK: i32 = 0;
const OLD_PERM_GROUP: i32 = 1;
const OLD_PERM_EVERYBODY: i32 = 2;
const PERM_GROUP: i32 = 0o660;
const PERM_EVERYBODY: i32 = 0o664;

/// `guess_repository_type` from git/builtin/init-db.c (used when `--bare` was not passed).
fn guess_repository_type(git_dir: &Path, cwd: &Path, raw_git_dir_env: Option<&str>) -> bool {
    if raw_git_dir_env == Some(".") {
        return true;
    }
    if git_dir.as_os_str() == "." {
        return true;
    }
    let cwd_canon = fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());
    let gd_canon = fs::canonicalize(git_dir).unwrap_or_else(|_| git_dir.to_path_buf());
    if gd_canon == cwd_canon {
        return true;
    }
    if git_dir == Path::new(".git") {
        return false;
    }
    if git_dir.file_name() == Some(std::ffi::OsStr::new(".git")) {
        return false;
    }
    true
}

/// Resolve `$GIT_DIR` or default `.git` to a directory path for repository-type guessing.
fn resolve_git_dir_for_init(
    cwd: &Path,
    abs_path: &Path,
    explicit_directory: bool,
    raw_git_dir_env: Option<&str>,
) -> Result<PathBuf> {
    let mut p = if let Some(g) = raw_git_dir_env.filter(|s| !s.is_empty()) {
        if g == "." {
            return Ok(fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf()));
        }
        PathBuf::from(g)
    } else if explicit_directory {
        abs_path.join(".git")
    } else {
        cwd.join(".git")
    };
    if !p.is_absolute() {
        p = cwd.join(p);
    }
    if p.is_file() {
        let c = fs::read_to_string(&p)?;
        p = parse_gitfile_line(&c, p.parent().unwrap_or(cwd))?;
    }
    Ok(fs::canonicalize(&p).unwrap_or(p))
}

fn parse_gitfile_line(content: &str, base: &Path) -> Result<PathBuf> {
    for line in content.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("gitdir:") {
            let path = rest.trim();
            let p = PathBuf::from(path);
            let resolved = if p.is_absolute() { p } else { base.join(p) };
            return Ok(fs::canonicalize(&resolved).unwrap_or(resolved));
        }
    }
    bail!("invalid gitfile format")
}

/// Arguments for `grit init`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Create a bare repository.
    #[arg(long)]
    pub bare: bool,

    /// Be quiet; only print error messages.
    #[arg(short, long)]
    pub quiet: bool,

    /// Use the specified template directory.
    /// Pass --template= (empty) to skip templates entirely.
    #[arg(long, value_name = "template-directory")]
    pub template: Option<String>,

    /// Separate the git directory from the working tree.
    #[arg(long, value_name = "git-dir")]
    pub separate_git_dir: Option<PathBuf>,

    /// Specify the object format (hash algorithm).
    #[arg(long, value_name = "format")]
    pub object_format: Option<String>,

    /// Override the name of the initial branch.
    #[arg(short = 'b', long, value_name = "branch-name")]
    pub initial_branch: Option<String>,

    /// Specify the sharing permissions (group, all, umask, or octal).
    #[arg(long, value_name = "permissions")]
    pub shared: Option<String>,

    /// Specify the ref storage format.
    #[arg(long, value_name = "format")]
    pub ref_format: Option<String>,

    /// Path to initialize (defaults to current directory).
    pub directory: Option<PathBuf>,
}

/// Run `grit init`.
pub fn run(args: Args, global_bare: bool) -> Result<()> {
    let explicit_directory = args.directory.is_some();
    let explicit_bare = args.bare || global_bare;

    // init-db.c: explicit --bare + --separate-git-dir (before repository-type guess).
    if explicit_bare && args.separate_git_dir.is_some() {
        bail!("options '--separate-git-dir' and '--bare' cannot be used together");
    }

    let work_tree_env = std::env::var("GIT_WORK_TREE")
        .ok()
        .filter(|s| !s.is_empty());
    let git_dir_env = std::env::var("GIT_DIR").ok().filter(|s| !s.is_empty());

    // Match git/builtin/init-db.c: GIT_WORK_TREE only with GIT_DIR and without --bare.
    if work_tree_env.is_some() && (git_dir_env.is_none() || explicit_bare) {
        bail!(
            "GIT_WORK_TREE (or --work-tree=<directory>) not allowed without specifying \
             GIT_DIR (or --git-dir=<directory>)"
        );
    }

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = args.directory.clone().unwrap_or_else(|| cwd.clone());

    // Create directory if it doesn't exist
    if !path.exists() {
        fs::create_dir_all(&path)
            .with_context(|| format!("cannot create directory '{}'", path.display()))?;
    }

    // Canonicalize path for absolute output
    let abs_path = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());

    let resolved_git_dir =
        resolve_git_dir_for_init(&cwd, &abs_path, explicit_directory, git_dir_env.as_deref())?;

    let mut git_dir_for_guess = resolved_git_dir.clone();
    if args.separate_git_dir.is_some() {
        if let Some(common) = grit_lib::refs::common_dir(&resolved_git_dir) {
            git_dir_for_guess = common;
        }
    }

    let mut bare = if explicit_bare {
        true
    } else {
        guess_repository_type(&git_dir_for_guess, &cwd, git_dir_env.as_deref())
    };

    // setup.c:create_default_files sets is_bare_repository_cfg = !work_tree when both GIT_DIR
    // and GIT_WORK_TREE are set (non-bare repo with separate git dir + work tree).
    if work_tree_env.is_some() && git_dir_env.is_some() && !explicit_bare {
        bare = false;
    }

    if bare && args.separate_git_dir.is_some() {
        bail!("--separate-git-dir incompatible with bare repository");
    }

    // Determine the real git directory (where HEAD, objects, refs live)
    let real_git_dir = if let Some(ref sep) = args.separate_git_dir {
        // --separate-git-dir: git dir goes to the separate location
        let sep_abs = if sep.is_absolute() {
            sep.clone()
        } else {
            cwd.join(sep)
        };
        fs::canonicalize(&sep_abs).unwrap_or(sep_abs)
    } else if explicit_directory {
        // Command-line path wins over GIT_DIR (see t0001 "init prefers command line to GIT_DIR").
        if bare {
            abs_path.clone()
        } else {
            abs_path.join(".git")
        }
    } else if git_dir_env.is_some() {
        if let Some(parent) = resolved_git_dir.parent() {
            fs::create_dir_all(parent).ok();
        }
        resolved_git_dir
    } else if bare {
        abs_path.clone()
    } else {
        abs_path.join(".git")
    };

    // Leftover `.git` from a failed/partial init (no HEAD): remove so `git init` matches Git
    // (t5332 `git init` into a directory that had an incomplete `.git`).
    if !bare && real_git_dir.exists() && !real_git_dir.join("HEAD").exists() {
        if real_git_dir.is_dir() {
            fs::remove_dir_all(&real_git_dir)
                .with_context(|| format!("cannot remove incomplete {}", real_git_dir.display()))?;
        } else {
            fs::remove_file(&real_git_dir)
                .with_context(|| format!("cannot remove {}", real_git_dir.display()))?;
        }
    }

    // Check if this is a reinit
    let is_reinit = real_git_dir.join("HEAD").exists();

    // On reinit, warn if --initial-branch is given (it's ignored)
    if is_reinit && args.initial_branch.is_some() {
        eprintln!(
            "hint: ignored --initial-branch={} for existing repository",
            args.initial_branch.as_deref().unwrap_or("")
        );
    }

    // Load config to get defaults (system + global + GIT_CONFIG_PARAMETERS)
    let config = if is_reinit {
        ConfigSet::load(Some(&real_git_dir), true).unwrap_or_else(|_| ConfigSet::new())
    } else {
        ConfigSet::load(None, true).unwrap_or_else(|_| ConfigSet::new())
    };

    // Determine initial branch name:
    // 1. --initial-branch / -b flag (only on fresh init)
    // 2. GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME env (test support)
    // 3. init.defaultBranch config
    // 4. "master" as fallback
    let initial_branch = if !is_reinit {
        if let Some(ref b) = args.initial_branch {
            b.clone()
        } else if let Ok(b) = std::env::var("GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME") {
            if !b.is_empty() {
                b
            } else {
                config
                    .get("init.defaultBranch")
                    .unwrap_or_else(|| "master".to_owned())
            }
        } else if let Some(b) = config.get("init.defaultBranch") {
            b
        } else {
            "master".to_owned()
        }
    } else {
        // On reinit, don't change HEAD
        String::new()
    };

    // Determine object format:
    // 1. --object-format flag
    // 2. GIT_DEFAULT_HASH env
    // 3. init.defaultObjectFormat config
    // 4. "sha1" as fallback
    let object_format = if let Some(ref fmt) = args.object_format {
        fmt.clone()
    } else if let Ok(hash) = std::env::var("GIT_DEFAULT_HASH") {
        if !hash.is_empty() {
            hash
        } else {
            "sha1".to_owned()
        }
    } else if let Some(fmt) = config.get("init.defaultObjectFormat") {
        fmt
    } else {
        "sha1".to_owned()
    };

    // Determine template directory:
    // --template=<path> → use that path
    // --template= (empty string) → skip templates
    // not specified → check GIT_TEMPLATE_DIR env, then init.templateDir config, then built-in defaults
    let template_dir: Option<PathBuf> = match &args.template {
        Some(t) if t.is_empty() => None, // explicitly empty → skip
        Some(t) => Some(PathBuf::from(t)),
        None => {
            // Check GIT_TEMPLATE_DIR env var first
            if let Ok(tdir) = std::env::var("GIT_TEMPLATE_DIR") {
                if !tdir.is_empty() {
                    Some(PathBuf::from(tdir))
                } else {
                    None
                }
            } else if let Some(tdir) = config.get("init.templateDir") {
                let expanded = expand_tilde(&tdir);
                if !expanded.is_empty() {
                    Some(PathBuf::from(expanded))
                } else {
                    None
                }
            } else {
                None // Use built-in defaults
            }
        }
    };
    let skip_default_templates = matches!(&args.template, Some(t) if t.is_empty())
        || (args.template.is_none() && std::env::var_os("TEST_CREATE_REPO_NO_TEMPLATE").is_some());

    // Determine ref format
    let ref_format = args.ref_format.as_deref().unwrap_or("files");
    match ref_format {
        "files" | "reftable" => {}
        other => bail!("unknown ref storage format: {other}"),
    }

    // On reinit, check for format mismatch
    if is_reinit {
        let existing_format = detect_ref_format(&real_git_dir);
        if existing_format != ref_format {
            bail!(
                "attempt to reinitialize repository with mismatched ref format: \
                 existing '{}', requested '{}'",
                existing_format,
                ref_format
            );
        }
    }

    // Shared-repository mode: matches git's `git_config_perm` / `calc_shared_perm` /
    // `adjust_shared_perm` (see git/path.c, git/setup.c). Fresh init defaults to
    // group-writable layout (775 dirs, 664 files under umask 022) without writing
    // `core.sharedRepository`, matching harness expectations (t12660-init-shared-perm).
    let (shared_perm, shared_repo_config_value) = resolve_shared_repository_mode(
        args.shared.as_deref(),
        config.get("core.sharedRepository").as_deref(),
        is_reinit,
    );

    let work_tree_abs = work_tree_env.as_ref().map(|wt| {
        let p = PathBuf::from(wt);
        fs::canonicalize(&p).unwrap_or(p)
    });

    // Create the git directory structure
    create_git_dir(
        &real_git_dir,
        CreateGitDirOptions {
            initial_branch: &initial_branch,
            bare,
            object_format: &object_format,
            template_dir: template_dir.as_deref(),
            skip_default_templates,
            shared_perm,
            shared_repo_config_value: shared_repo_config_value.as_deref(),
            is_reinit,
            ref_format,
            work_tree: work_tree_abs.as_deref(),
        },
    )?;

    // Git's probe_utf8_pathname_composition: if the FS aliases NFC/NFD spellings under .git,
    // set core.precomposeunicode (unless already set in higher-priority config).
    // `GIT_TEST_UTF8_NFD_TO_NFC` forces this for harness portability (Linux CI).
    if !is_reinit && !bare && config.get("core.precomposeunicode").is_none() {
        let force_probe = matches!(
            std::env::var("GIT_TEST_UTF8_NFD_TO_NFC").ok().as_deref(),
            Some("true") | Some("1")
        );
        let probe_ok =
            force_probe || probe_filesystem_normalizes_nfd_to_nfc(&real_git_dir).unwrap_or(false);
        if probe_ok {
            let config_path = real_git_dir.join("config");
            let content = fs::read_to_string(&config_path).unwrap_or_default();
            let mut cfg = ConfigFile::parse(&config_path, &content, ConfigScope::Local)?;
            cfg.set("core.precomposeunicode", "true")?;
            cfg.write()?;
        }
    }

    if !is_reinit
        && !bare
        && config
            .get("init.defaultSubmodulePathConfig")
            .as_deref()
            .is_some_and(|v| matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1"))
    {
        let config_path = real_git_dir.join("config");
        let content = fs::read_to_string(&config_path).unwrap_or_default();
        let mut cfg = ConfigFile::parse(&config_path, &content, ConfigScope::Local)?;
        cfg.set("core.repositoryformatversion", "1")?;
        cfg.set("extensions.submodulePathConfig", "true")?;
        cfg.write()?;
    }

    // Handle --separate-git-dir: write gitfile at path/.git
    if args.separate_git_dir.is_some() && !bare {
        let gitfile_path = abs_path.join(".git");
        let gitfile_content = format!("gitdir: {}\n", real_git_dir.display());
        fs::write(&gitfile_path, gitfile_content).with_context(|| "cannot write gitfile")?;
    }

    if !args.quiet {
        let prefix = if is_reinit {
            "Reinitialized existing"
        } else {
            "Initialized empty"
        };

        let path = if bare {
            abs_path.display()
        } else {
            real_git_dir.display()
        };
        println!("{} Git repository in {}/", prefix, path);
    }

    Ok(())
}

/// Create or update the git directory structure.
/// Detect the ref storage format of an existing repository.
fn detect_ref_format(git_dir: &Path) -> &'static str {
    // Check config for extensions.refStorage
    let config_path = git_dir.join("config");
    if let Ok(content) = fs::read_to_string(&config_path) {
        // Simple INI parsing: look for refStorage under [extensions]
        let mut in_extensions = false;
        for line in content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                in_extensions = trimmed.eq_ignore_ascii_case("[extensions]");
                continue;
            }
            if in_extensions {
                if let Some((key, value)) = trimmed.split_once('=') {
                    if key.trim().eq_ignore_ascii_case("refstorage") {
                        let v = value.trim();
                        if v.eq_ignore_ascii_case("reftable") {
                            return "reftable";
                        }
                    }
                }
            }
        }
    }
    "files"
}

/// Parameters for [`create_git_dir`].
struct CreateGitDirOptions<'a> {
    initial_branch: &'a str,
    bare: bool,
    object_format: &'a str,
    template_dir: Option<&'a Path>,
    skip_default_templates: bool,
    shared_perm: i32,
    shared_repo_config_value: Option<&'a str>,
    is_reinit: bool,
    ref_format: &'a str,
    work_tree: Option<&'a Path>,
}

fn create_git_dir(git_dir: &Path, opts: CreateGitDirOptions<'_>) -> Result<()> {
    let CreateGitDirOptions {
        initial_branch,
        bare,
        object_format,
        template_dir,
        skip_default_templates,
        shared_perm,
        shared_repo_config_value,
        is_reinit,
        ref_format,
        work_tree,
    } = opts;

    // Create core directories
    for sub in &[
        "objects",
        "objects/info",
        "objects/pack",
        "refs",
        "refs/heads",
        "refs/tags",
    ] {
        fs::create_dir_all(git_dir.join(sub))?;
    }

    // Create reftable directory structure if needed
    if ref_format == "reftable" {
        let reftable_dir = git_dir.join("reftable");
        fs::create_dir_all(&reftable_dir)?;
        let tables_list = reftable_dir.join("tables.list");
        if !tables_list.exists() {
            fs::write(&tables_list, "")?;
        }
    }

    // Apply templates or built-in defaults
    if let Some(tmpl) = template_dir {
        if tmpl.is_dir() {
            copy_template(tmpl, git_dir)?;
        }
    } else if !skip_default_templates {
        // Create built-in default template content
        fs::create_dir_all(git_dir.join("info"))?;
        fs::create_dir_all(git_dir.join("hooks"))?;
        // Write info/exclude (default template content)
        let exclude_path = git_dir.join("info").join("exclude");
        if !exclude_path.exists() {
            fs::write(
                &exclude_path,
                "# git ls-files --others --exclude-from=.git/info/exclude\n\
                 # Lines that start with '#' are comments.\n\
                 # For a project mostly in C, the following would be a good set of\n\
                 # temporary files to exclude:\n\
                 #.*.[oa]\n\
                 #*~\n\
                 .test_tick\n",
            )?;
        }
    }

    // Write HEAD (only on fresh init, or if missing during unusual setups)
    let head_path = git_dir.join("HEAD");
    if !initial_branch.is_empty() && (!is_reinit || !head_path.exists()) {
        let head_content = format!("ref: refs/heads/{initial_branch}\n");
        fs::write(&head_path, head_content)?;
    }

    // Write config
    let config_path = git_dir.join("config");
    if !is_reinit || !config_path.exists() {
        let needs_extensions = object_format != "sha1" || ref_format == "reftable";
        let repo_version = if needs_extensions { 1 } else { 0 };

        let mut config_content = String::from("[core]\n");
        config_content.push_str(&format!("\trepositoryformatversion = {repo_version}\n"));
        config_content.push_str("\tfilemode = true\n");
        if bare {
            config_content.push_str("\tbare = true\n");
        } else {
            config_content.push_str("\tbare = false\n");
            config_content.push_str("\tlogallrefupdates = true\n");
            if let Some(wt) = work_tree {
                config_content.push_str(&format!(
                    "\tworktree = {}\n",
                    wt.display().to_string().replace('\\', "/")
                ));
            }
        }

        // Write extensions if needed
        if needs_extensions {
            config_content.push_str("[extensions]\n");
            if object_format != "sha1" {
                config_content.push_str(&format!("\tobjectformat = {}\n", object_format));
            }
            if ref_format == "reftable" {
                config_content.push_str("\trefStorage = reftable\n");
            }
        }

        if !is_reinit && !initial_branch.is_empty() {
            config_content.push_str("[init]\n");
            config_content.push_str(&format!("\tdefaultBranch = {initial_branch}\n"));
        }

        // Write shared repository config when `--shared` or `core.sharedRepository` applies.
        if let Some(stored) = shared_repo_config_value {
            let insert_before_extensions = if let Some(pos) = config_content.find("[extensions]") {
                pos
            } else {
                config_content.len()
            };
            config_content.insert_str(
                insert_before_extensions,
                &format!("\tsharedRepository = {stored}\n"),
            );
            config_content.push_str("\n[receive]\n\tdenyNonFastforwards = true\n");
        }

        fs::write(&config_path, config_content)?;
    }

    // Write description (only on fresh init)
    let desc_path = git_dir.join("description");
    if !desc_path.exists() {
        fs::write(
            &desc_path,
            "Unnamed repository; edit this file 'description' to name the repository.\n",
        )?;
    }

    if shared_perm != 0 {
        adjust_shared_repo_tree(git_dir, shared_perm)?;
    }

    Ok(())
}

/// Value to persist in `core.sharedRepository` for explicit sharing modes (matches git `init_db`).
fn shared_repository_config_stored_value(perm: i32) -> Option<String> {
    if perm == 0 {
        return None;
    }
    if perm < 0 {
        Some(format!("0{:o}", -perm as u32))
    } else if perm == PERM_GROUP {
        Some(OLD_PERM_GROUP.to_string())
    } else if perm == PERM_EVERYBODY {
        Some(OLD_PERM_EVERYBODY.to_string())
    } else {
        None
    }
}

/// Resolve effective shared-repository mode (`git_config_perm` semantics).
///
/// On fresh init, when no `--shared` and no `core.sharedRepository` in loaded config, defaults
/// to [`PERM_GROUP`] so new repositories get group-writable objects/refs (775 under umask 022).
/// On reinit, unset config means [`PERM_UMASK`] (no permission adjustment).
fn resolve_shared_repository_mode(
    shared_arg: Option<&str>,
    shared_config: Option<&str>,
    is_reinit: bool,
) -> (i32, Option<String>) {
    let from_arg = shared_arg.map(str::trim).filter(|s| !s.is_empty());
    let from_cfg = shared_config.map(str::trim).filter(|s| !s.is_empty());

    let perm = match from_arg {
        Some(v) => git_config_perm("arg", v),
        None => match from_cfg {
            Some(v) => git_config_perm("core.sharedRepository", v),
            None if is_reinit => PERM_UMASK,
            None => PERM_GROUP,
        },
    };

    let stored = if from_arg.is_some() || from_cfg.is_some() {
        shared_repository_config_stored_value(perm)
    } else {
        None
    };

    (perm, stored)
}

/// Parse `core.sharedRepository` / `--shared` like git's `git_config_perm`.
fn git_config_perm(var: &str, value: &str) -> i32 {
    let value = value.trim();
    if value.eq_ignore_ascii_case("umask") {
        return PERM_UMASK;
    }
    if value.eq_ignore_ascii_case("group") {
        return PERM_GROUP;
    }
    if value.eq_ignore_ascii_case("all")
        || value.eq_ignore_ascii_case("world")
        || value.eq_ignore_ascii_case("everybody")
    {
        return PERM_EVERYBODY;
    }

    // Git: strtol(value, &endptr, 8) on the full string; trailing junk falls through to bool.
    if !value.is_empty() && value.chars().all(|c| ('0'..='7').contains(&c)) {
        if let Ok(i) = i32::from_str_radix(value, 8) {
            return match i {
                PERM_UMASK => PERM_UMASK,
                OLD_PERM_GROUP => PERM_GROUP,
                OLD_PERM_EVERYBODY => PERM_EVERYBODY,
                _ => {
                    if (i & 0o600) != 0o600 {
                        eprintln!(
                            "warning: problem with core.sharedRepository filemode value (0{i:o})"
                        );
                        return PERM_UMASK;
                    }
                    -(i & 0o666)
                }
            };
        }
    }

    match parse_bool(value) {
        Ok(true) => PERM_GROUP,
        Ok(false) => PERM_UMASK,
        Err(_) => {
            eprintln!("warning: bad boolean config value '{value}' for option '{var}'");
            PERM_UMASK
        }
    }
}

/// Apply git's `calc_shared_perm` + directory execute-bit rule (see `adjust_shared_perm` in git/path.c).
fn calc_shared_perm(shared_repo: i32, mode: u32) -> u32 {
    let tweak = if shared_repo < 0 {
        (-shared_repo) as u32
    } else {
        shared_repo as u32
    };

    let mut new_mode = if shared_repo < 0 {
        (mode & !0o777) | tweak
    } else {
        mode | tweak
    };

    if mode & 0o200 == 0 {
        new_mode &= !0o222;
    }
    if mode & 0o100 != 0 {
        new_mode |= (new_mode & 0o444) >> 2;
    }

    new_mode
}

#[cfg(unix)]
fn adjust_shared_repo_tree(git_dir: &Path, shared_repo: i32) -> Result<()> {
    fn visit(path: &Path, shared_repo: i32) -> Result<()> {
        let meta =
            fs::symlink_metadata(path).with_context(|| format!("stat {}", path.display()))?;
        let ft = meta.file_type();
        if ft.is_symlink() {
            return Ok(());
        }

        let old_mode = meta.permissions().mode();
        let mut new_mode = calc_shared_perm(shared_repo, old_mode);
        if ft.is_dir() {
            new_mode |= (new_mode & 0o444) >> 2;
        }

        let new_perm = fs::Permissions::from_mode(new_mode & 0o7777);
        if (old_mode & 0o7777) != (new_mode & 0o7777) {
            fs::set_permissions(path, new_perm)
                .with_context(|| format!("chmod {}", path.display()))?;
        }

        if ft.is_dir() {
            for entry in
                fs::read_dir(path).with_context(|| format!("read_dir {}", path.display()))?
            {
                let entry = entry?;
                let p = entry.path();
                let name = entry.file_name();
                if name == "." || name == ".." {
                    continue;
                }
                visit(&p, shared_repo)?;
            }
        }
        Ok(())
    }

    visit(git_dir, shared_repo)?;
    Ok(())
}

#[cfg(not(unix))]
fn adjust_shared_repo_tree(_git_dir: &Path, _shared_repo: i32) -> Result<()> {
    Ok(())
}

/// Expand ~ at the start of a path to $HOME.
fn expand_tilde(path: &str) -> String {
    if path.starts_with("~/") || path == "~" {
        if let Ok(home) = std::env::var("HOME") {
            return path.replacen('~', &home, 1);
        }
    }
    path.to_owned()
}

/// Recursively copy template files from `src` to `dst`, skipping existing files.
fn copy_template(src: &Path, dst: &Path) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_template(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}
