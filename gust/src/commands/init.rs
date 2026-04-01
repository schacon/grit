//! `gust init` — initialise or reinitialise a Git repository.

use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use std::path::PathBuf;

use gust_lib::repo::init_repository;

/// Arguments for `gust init`.
#[derive(Debug, ClapArgs)]
pub struct Args {
    /// Create a bare repository.
    #[arg(long)]
    pub bare: bool,

    /// Be quiet; only print error messages.
    #[arg(short, long)]
    pub quiet: bool,

    /// Use the specified template directory.
    #[arg(long, value_name = "template-directory")]
    pub template: Option<PathBuf>,

    /// Separate the git directory from the working tree.
    #[arg(long, value_name = "git-dir")]
    pub separate_git_dir: Option<PathBuf>,

    /// Specify the object format (only 'sha1' supported in v1).
    #[arg(long, value_name = "format", default_value = "sha1")]
    pub object_format: String,

    /// Override the name of the initial branch.
    #[arg(short = 'b', long, value_name = "branch-name")]
    pub initial_branch: Option<String>,

    /// Path to initialize (defaults to current directory).
    pub directory: Option<PathBuf>,
}

/// Run `gust init`.
pub fn run(args: Args) -> Result<()> {
    let path = args
        .directory
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    // Create directory if it doesn't exist
    if !path.exists() {
        std::fs::create_dir_all(&path)
            .with_context(|| format!("cannot create directory '{}'", path.display()))?;
    }

    let initial_branch = args.initial_branch.as_deref().unwrap_or("master");
    let template = args.template.as_deref();

    let repo = init_repository(&path, args.bare, initial_branch, template)
        .with_context(|| format!("failed to initialize repository at '{}'", path.display()))?;

    // Handle --separate-git-dir: write a gitfile in the work tree
    if let Some(sep) = &args.separate_git_dir {
        if !args.bare {
            let git_file = path.join(".git");
            let target = sep.canonicalize().unwrap_or_else(|_| sep.clone());
            std::fs::write(&git_file, format!("gitdir: {}\n", target.display()))
                .with_context(|| "cannot write gitfile")?;
        }
    }

    if !args.quiet {
        let git_dir = &repo.git_dir;
        println!("Initialized empty Git repository in {}/", git_dir.display());
    }

    Ok(())
}
