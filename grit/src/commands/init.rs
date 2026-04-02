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

    /// Path to initialize (defaults to current directory).
    pub directory: Option<PathBuf>,
}

/// Run `grit init`.
pub fn run(args: Args, global_bare: bool) -> Result<()> {
    let bare = args.bare || global_bare;

    // --bare and --separate-git-dir are incompatible
    if bare && args.separate_git_dir.is_some() {
        bail!("--bare and --separate-git-dir are incompatible");
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
    let abs_path = fs::canonicalize(&path)
        .unwrap_or_else(|_| path.clone());

    // Determine the git directory
    let git_dir = if bare {
        abs_path.clone()
    } else {
        abs_path.join(".git")
    };

    // Check if this is a reinit
    let is_reinit = git_dir.join("HEAD").exists();

    // Load config to get defaults (system + global + GIT_CONFIG_PARAMETERS)
    // For init, we don't have a local config yet (or we do on reinit)
    let config = if is_reinit {
        ConfigSet::load(Some(&git_dir), true).unwrap_or_else(|_| ConfigSet::new())
    } else {
        ConfigSet::load(None, true).unwrap_or_else(|_| ConfigSet::new())
    };

    // Determine initial branch name:
    // 1. --initial-branch / -b flag
    // 2. init.defaultBranch config
    // 3. "master" as fallback
    let initial_branch = if let Some(ref b) = args.initial_branch {
        b.clone()
    } else if let Some(b) = config.get("init.defaultBranch") {
        b
    } else {
        "master".to_owned()
    };

    // Determine object format:
    // 1. --object-format flag
    // 2. GIT_DEFAULT_HASH env
    // 3. init.defaultObjectFormat config
    // 4. "sha1" as fallback
    let object_format = if let Some(ref fmt) = args.object_format {
        fmt.clone()
    } else if let Ok(hash) = std::env::var("GIT_DEFAULT_HASH") {
        hash
    } else if let Some(fmt) = config.get("init.defaultObjectFormat") {
        fmt
    } else {
        "sha1".to_owned()
    };

    // Determine template directory:
    // --template=<path> → use that path
    // --template= (empty string) → skip templates
    // not specified → check init.templateDir config, then use built-in defaults
    let template_dir: Option<PathBuf> = match &args.template {
        Some(t) if t.is_empty() => None, // explicitly empty → skip
        Some(t) => Some(PathBuf::from(t)),
        None => {
            // Check config for init.templateDir
            if let Some(tdir) = config.get("init.templateDir") {
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
    let skip_default_templates = matches!(&args.template, Some(t) if t.is_empty());

    // Create the git directory structure
    create_git_dir(&git_dir, &initial_branch, bare, &object_format,
                   template_dir.as_deref(), skip_default_templates,
                   args.shared.as_deref(), is_reinit)?;

    // Handle --separate-git-dir
    if let Some(sep) = &args.separate_git_dir {
        if !bare {
            // Move git dir content to the separate location
            let sep_abs = if sep.is_absolute() {
                sep.clone()
            } else {
                std::env::current_dir()?.join(sep)
            };
            if !sep_abs.exists() {
                fs::create_dir_all(&sep_abs)?;
            }
            // If we just created git_dir, move its contents to sep_abs
            // and write a gitfile in the original location
            move_git_dir(&git_dir, &sep_abs)?;
            // Write gitfile
            let gitfile_content = format!("gitdir: {}\n", sep_abs.display());
            fs::write(&git_dir, gitfile_content)
                .with_context(|| "cannot write gitfile")?;
        }
    }

    if !args.quiet {
        let prefix = if is_reinit {
            "Reinitialized existing"
        } else {
            "Initialized empty"
        };

        if bare {
            println!("{} Git repository in {}/", prefix, abs_path.display());
        } else {
            println!("{} Git repository in {}/", prefix, git_dir.display());
        }
    }

    Ok(())
}

/// Create or update the git directory structure.
fn create_git_dir(
    git_dir: &Path,
    initial_branch: &str,
    bare: bool,
    object_format: &str,
    template_dir: Option<&Path>,
    skip_default_templates: bool,
    shared: Option<&str>,
    is_reinit: bool,
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
                 #*~\n",
            )?;
        }
    }

    // Write HEAD (only if not reinit, or if it doesn't exist)
    let head_path = git_dir.join("HEAD");
    if !head_path.exists() {
        let head_content = format!("ref: refs/heads/{initial_branch}\n");
        fs::write(&head_path, head_content)?;
    }

    // Write config
    let config_path = git_dir.join("config");
    if !is_reinit || !config_path.exists() {
        let mut config_content = String::from("[core]\n");
        config_content.push_str("\trepositoryformatversion = 0\n");
        config_content.push_str("\tfilemode = true\n");
        if bare {
            config_content.push_str("\tbare = true\n");
        } else {
            config_content.push_str("\tbare = false\n");
            config_content.push_str("\tlogallrefupdates = true\n");
        }

        // Write object format extension if not sha1
        if object_format != "sha1" {
            // Bump repository format version to 1 for extensions
            config_content = config_content.replace(
                "repositoryformatversion = 0",
                "repositoryformatversion = 1",
            );
            config_content.push_str(&format!(
                "[extensions]\n\tobjectformat = {}\n",
                object_format
            ));
        }

        // Write shared repository config
        if let Some(perm) = shared {
            let shared_value = normalize_shared(perm);
            // Insert sharedRepository into existing [core] section
            // Find the last line of core section content and append there
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
        other => {
            // If it's an octal number, keep it as-is (e.g. "0666")
            other.to_owned()
        }
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
        } else if !dst_path.exists() {
            fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// Move contents of `src` directory to `dst`, then remove `src`.
fn move_git_dir(src: &Path, dst: &Path) -> Result<()> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if src_path.is_dir() {
            fs::create_dir_all(&dst_path)?;
            move_git_dir(&src_path, &dst_path)?;
            fs::remove_dir(&src_path)?;
        } else {
            fs::rename(&src_path, &dst_path)?;
        }
    }
    // Remove the now-empty source directory
    let _ = fs::remove_dir(src);
    Ok(())
}
