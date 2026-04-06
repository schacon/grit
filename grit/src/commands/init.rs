//! `grit init` — initialise or reinitialise a Git repository.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use std::fs;
use std::path::{Path, PathBuf};

use grit_lib::config::ConfigSet;

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
    let bare = args.bare || global_bare;

    // --bare and --separate-git-dir are incompatible
    if bare && args.separate_git_dir.is_some() {
        bail!("options '--bare' and '--separate-git-dir' cannot be used together");
    }

    let path = args
        .directory
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // Create directory if it doesn't exist
    if !path.exists() {
        fs::create_dir_all(&path)
            .with_context(|| format!("cannot create directory '{}'", path.display()))?;
    }

    // Canonicalize path for absolute output
    let abs_path = fs::canonicalize(&path).unwrap_or_else(|_| path.clone());

    // Determine the real git directory (where HEAD, objects, refs live)
    let real_git_dir = if let Some(ref sep) = args.separate_git_dir {
        // --separate-git-dir: git dir goes to the separate location
        let sep_abs = if sep.is_absolute() {
            sep.clone()
        } else {
            std::env::current_dir()?.join(sep)
        };
        fs::canonicalize(&sep_abs).unwrap_or(sep_abs)
    } else if let Ok(env_git_dir) = std::env::var("GIT_DIR") {
        // Respect GIT_DIR env var (set by --git-dir global option)
        let gd = PathBuf::from(&env_git_dir);
        let gd_abs = if gd.is_absolute() {
            gd
        } else {
            std::env::current_dir()?.join(gd)
        };
        // Ensure parent directory exists
        if let Some(parent) = gd_abs.parent() {
            fs::create_dir_all(parent).ok();
        }
        gd_abs
    } else if bare {
        abs_path.clone()
    } else {
        abs_path.join(".git")
    };

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

    let test_no_template = std::env::var("TEST_CREATE_REPO_NO_TEMPLATE")
        .ok()
        .is_some_and(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "on"));

    // Determine template directory:
    // --template=<path> → use that path
    // --template= (empty string) → skip templates
    // not specified → check GIT_TEMPLATE_DIR env, then init.templateDir config, then built-in defaults
    let template_dir: Option<PathBuf> = match &args.template {
        Some(t) if t.is_empty() => None, // explicitly empty → skip
        Some(t) => Some(PathBuf::from(t)),
        None => {
            if test_no_template {
                None
            } else
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
    let skip_default_templates =
        matches!(&args.template, Some(t) if t.is_empty()) || test_no_template;

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

    // Create the git directory structure
    create_git_dir(
        &real_git_dir,
        &initial_branch,
        bare,
        &object_format,
        template_dir.as_deref(),
        skip_default_templates,
        args.shared.as_deref(),
        is_reinit,
        ref_format,
    )?;

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

        if bare {
            println!("{} Git repository in {}/", prefix, abs_path.display());
        } else if args.separate_git_dir.is_some() {
            println!("{} Git repository in {}/", prefix, real_git_dir.display());
        } else {
            println!("{} Git repository in {}/", prefix, real_git_dir.display());
        }
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

fn create_git_dir(
    git_dir: &Path,
    initial_branch: &str,
    bare: bool,
    object_format: &str,
    template_dir: Option<&Path>,
    skip_default_templates: bool,
    shared: Option<&str>,
    is_reinit: bool,
    ref_format: &str,
) -> Result<()> {
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

    // Always create essential directories (hooks, info) regardless of template.
    // Git does this unconditionally so that hook installation always works.
    fs::create_dir_all(git_dir.join("info"))?;
    fs::create_dir_all(git_dir.join("hooks"))?;

    // Apply templates or built-in defaults
    if let Some(tmpl) = template_dir {
        if tmpl.is_dir() {
            copy_template(tmpl, git_dir)?;
        }
    } else if !skip_default_templates {
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
                 #*~\n",
            )?;
        }
    }

    // Write HEAD (only on fresh init)
    let head_path = git_dir.join("HEAD");
    if !is_reinit && !initial_branch.is_empty() {
        let head_content = format!("ref: refs/heads/{initial_branch}\n");
        fs::write(&head_path, head_content)?;
    } else if !head_path.exists() && !initial_branch.is_empty() {
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

        // Write shared repository config in [core] section
        if let Some(perm) = shared {
            let shared_value = normalize_shared(perm);
            let insert_before_extensions = if let Some(pos) = config_content.find("[extensions]") {
                pos
            } else {
                config_content.len()
            };
            config_content.insert_str(
                insert_before_extensions,
                &format!("\tsharedRepository = {}\n", shared_value),
            );
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

    Ok(())
}

/// Normalize --shared value to what git stores in config.
fn normalize_shared(perm: &str) -> String {
    match perm {
        "group" | "true" => "1".to_owned(),
        "all" | "world" | "everybody" => "2".to_owned(),
        "umask" | "false" => "0".to_owned(),
        other => other.to_owned(),
    }
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
