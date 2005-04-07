fn run_update_inner(args: &UpdateArgs, discover_base: Option<PathBuf>) -> Result<()> {
    if args.filter.is_some() && !args.init {
        eprintln!("usage: git submodule update [--init] [--filter=<filter-spec>]");
        std::process::exit(129);
    }

    let repo = if let Some(ref p) = discover_base {
        Repository::discover(Some(p)).context("not a git repository")?
    } else {
        Repository::discover(None).context("not a git repository")?
    };
    let work_tree = repo.work_tree.as_ref().context("bare repository")?;
    let super_shallow = repo.git_dir.join("shallow").is_file();
    let mut reference_roots: Vec<PathBuf> = Vec::new();
    for r in &args.reference {
        let p = Path::new(r);
        let resolved = if p.is_absolute() {
            p.to_path_buf()
        } else {
            work_tree.join(p)
        };
        reference_roots.push(resolved);
    }

    trace_submodule_job_tasks_if_needed(args.jobs);

    if args.init {
        init_in_repo(
            &repo,
            &InitArgs {
                quiet: false,
                paths: args.paths.clone(),
            },
            args.quiet,
        )?;
    }

    let modules = parse_gitmodules_with_repo(work_tree, Some(&repo))?;
    let selected = filter_submodules(&modules, &args.paths);

    let grit_bin = grit_exe::grit_executable();
    let super_index = repo.load_index().ok();
    let local_cfg = parse_local_config(&repo.git_dir)?;

    let mut any_failure = false;

    for m in &selected {
        let sub_path = work_tree.join(&m.path);
        let msg_path = submodule_display_path_from_cwd(&sub_path);

        if super_index_has_unmerged_stage(&repo, &m.path) {
            if args.paths.is_empty() {
                eprintln!("Skipping unmerged submodule {}", msg_path);
            }
            continue;
        }

        let update_key = format!("submodule.{}.update", m.name);
        let update_strategy = config_last_value(&local_cfg, &update_key)
            .or_else(|| m.update.clone())
            .unwrap_or_default();

        if update_strategy == "none" && !args.checkout && !args.remote {
            continue;
        }

        let url_key = format!("submodule.{}.url", m.name);
        let has_url = local_cfg.entries.iter().any(|e| e.key == url_key);
        if !has_url {
            if !args.paths.is_empty() {
                eprintln!("Submodule path '{msg_path}' not initialized");
            } else {
                eprintln!(
                    "Skipping submodule '{}': not initialized (run 'grit submodule init')",
                    m.path
                );
            }
            continue;
        }

        let path_bytes = m.path.as_bytes();
        let recorded_from_index = super_index
            .as_ref()
            .and_then(|idx| idx.get(path_bytes, 0))
            .filter(|e| e.mode == 0o160000)
            .map(|e| e.oid.to_hex());
        let recorded = if recorded_from_index.is_some() {
            recorded_from_index
        } else {
            read_submodule_commit(&repo, &m.path)?
        };
        let Some(recorded_oid) = recorded.as_deref() else {
            continue;
        };

        let modules_dir = submodule_modules_git_dir(&repo.git_dir, &m.name);

        // When `--recursive` is set, do not skip submodules that are already at the recorded
        // commit: nested `.gitmodules` paths may still need `submodule update` (t5531 nested clone).
        if !args.remote
            && !args.force
            && !args.recursive
            && sub_path.join(".git").exists()
            && read_submodule_head(&sub_path).as_deref() == Some(recorded_oid)
        {
            continue;
        }

        if sub_path.join(".git").is_dir() && modules_dir.join("HEAD").exists() {
            fs::remove_dir_all(sub_path.join(".git")).with_context(|| {
                format!(
                    "failed to normalize submodule .git at {}",
                    sub_path.display()
                )
            })?;
            attach_existing_submodule_worktree(&grit_bin, &modules_dir, &sub_path)?;
        }

        if args.init && sub_path.join(".git").exists() {
            if let Ok(nested_repo) = Repository::open(&modules_dir, Some(&sub_path))
                .or_else(|_| Repository::discover(Some(&sub_path)))
            {
                let _ = init_in_repo(&nested_repo, &InitArgs { quiet: false, paths: vec![] }, false);
            }
        }

        let needs_clone = if sub_path.exists() {
            !sub_path.join(".git").exists()
        } else {
            true
        };

        if needs_clone {
            if modules_dir.join("HEAD").exists() {
                attach_existing_submodule_worktree(&grit_bin, &modules_dir, &sub_path)?;
            } else {
                if let Some(parent) = modules_dir.parent() {
                    fs::create_dir_all(parent)?;
                }
                if modules_dir.exists() {
                    let _ = fs::remove_dir(&modules_dir);
                }

                let clone_url =
                    config_last_value(&local_cfg, &url_key).unwrap_or_else(|| m.url.clone());

                if sub_path.exists() {
                    let _ = fs::remove_dir_all(&sub_path);
                }

                let clone_src_str =
                    resolve_submodule_super_url(work_tree, &repo.git_dir, &clone_url)?;

                let mut clone_cmd = grit_subprocess(&grit_bin);
                clone_cmd
                    .arg("clone")
                    .arg("--no-checkout")
                    .arg("--separate-git-dir")
                    .arg(&modules_dir)
                    .arg(&clone_src_str)
                    .arg(&sub_path)
                    .current_dir(work_tree)
                    .env_remove("GIT_DIR")
                    .env_remove("GIT_WORK_TREE");
                if args.quiet {
                    clone_cmd.arg("--quiet");
                }
                let depth_clone = args.depth.or_else(|| {
                    submodule_clone_depth_for_superproject(
                        super_shallow,
                        clone_shallow_submodules_from_env(),
                        clone_no_shallow_submodules_from_env(),
                        args.no_recommend_shallow,
                        m.shallow,
                    )
                });
                if let Some(d) = depth_clone {
                    clone_cmd.arg("--depth").arg(d.to_string());
                }
                if let Some(ref f) = args.filter {
                    clone_cmd.arg("--filter").arg(f);
                }

                let status = clone_cmd.status().context("failed to clone submodule")?;

                if !status.success() {
                    eprintln!("error: failed to clone submodule from '{clone_src_str}'");
                    bail!("failed to clone submodule '{}'", m.name);
                }
                set_submodule_core_worktree(&grit_bin, &modules_dir, &sub_path);

                if !reference_roots.is_empty() {
                    write_submodule_object_alternates(
                        &modules_dir,
                        &repo.git_dir,
                        &reference_roots,
                    )?;
                }

                if args.init {
                    let nested_repo = Repository::open(&modules_dir, Some(&sub_path))
                        .or_else(|_| Repository::discover(Some(&sub_path)))?;
                    init_in_repo(&nested_repo, &InitArgs { quiet: false, paths: vec![] }, false)?;
                }

                let abs_origin = if Path::new(&clone_src_str).is_absolute() {
                    Path::new(&clone_src_str).canonicalize()
                } else {
                    work_tree.join(&clone_src_str).canonicalize()
                };
                if let Ok(p) = abs_origin {
                    let sub_cfg_path = modules_dir.join("config");
                    if sub_cfg_path.exists() {
                        let sub_content = fs::read_to_string(&sub_cfg_path)?;
                        let mut sub_cfg =
                            ConfigFile::parse(&sub_cfg_path, &sub_content, ConfigScope::Local)?;
                        sub_cfg.set("remote.origin.url", &p.to_string_lossy())?;
                        sub_cfg.write()?;
                    }
                }
            }
        }

        let shell_update = config_last_value(&local_cfg, &update_key)
            .filter(|u| u.trim_start().starts_with('!'));

        if let Some(cmd_tpl) = shell_update {
            let expanded =
                expand_submodule_shell_command(cmd_tpl.as_str(), recorded_oid, &m.path, work_tree);
            let inner = expanded
                .strip_prefix('!')
                .map(str::trim)
                .unwrap_or(expanded.as_str());
            let st = std::process::Command::new("sh")
                .arg("-lc")
                .arg(inner)
                .current_dir(&sub_path)
                .status()
                .context("run submodule update command")?;
            if !st.success() {
                bail!(
                    "fatal: Execution of '{}' failed in submodule path '{}'",
                    inner,
                    msg_path
                );
            }
        } else {
            let checkout_oid = if args.remote {
                let nested_cfg_path = if sub_path.join(".git").is_file() {
                    modules_dir.join("config")
                } else {
                    sub_path.join(".git").join("config")
                };
                let nested_content = fs::read_to_string(&nested_cfg_path)?;
                let nested_cfg = ConfigFile::parse(&nested_cfg_path, &nested_content, ConfigScope::Local)?;
                let used_local_fetch =
                    submodule_fetch_origin_local_path(&sub_path, &nested_cfg, args.quiet)?;
                if !used_local_fetch {
                    let mut fetch_cmd = grit_subprocess(&grit_bin);
                    fetch_cmd
                        .args(["fetch", "origin"])
                        .current_dir(&sub_path)
                        .env_remove("GIT_DIR")
                        .env_remove("GIT_WORK_TREE");
                    if args.quiet {
                        fetch_cmd.arg("--quiet");
                    }
                    let fetch_status = fetch_cmd.status().context("failed to fetch in submodule")?;
                    if !fetch_status.success() {
                        bail!("failed to fetch in submodule '{}'", m.name);
                    }
                }

                let explicit_branch = {
                    let key = format!("submodule.{}.branch", m.name);
                    config_last_value(&local_cfg, &key).or_else(|| m.branch.clone())
                };

                let mut output = if let Some(branch) = explicit_branch {
                    let b = if branch == "." {
                        superproject_head_short_branch(&repo)
                            .unwrap_or_else(default_initial_branch_name)
                    } else {
                        branch
                    };
                    grit_subprocess(&grit_bin)
                        .args(["rev-parse", &format!("origin/{b}")])
                        .current_dir(&sub_path)
                        .output()
                        .context("failed to resolve remote tracking branch")?
                } else {
                    grit_subprocess(&grit_bin)
                        .args(["rev-parse", "origin/HEAD"])
                        .current_dir(&sub_path)
                        .output()
                        .context("rev-parse origin/HEAD")?
                };

                if !output.status.success() {
                    let branch = resolve_submodule_remote_branch_name(&repo, m, &local_cfg);
                    output = grit_subprocess(&grit_bin)
                        .args(["rev-parse", &format!("origin/{branch}")])
                        .current_dir(&sub_path)
                        .output()
                        .context("failed to resolve remote tracking branch (fallback)")?;
                }

                if !output.status.success() {
                    bail!("failed to resolve remote tip in submodule '{}'", m.name);
                }
                String::from_utf8_lossy(&output.stdout).trim().to_string()
            } else {
                recorded_oid.to_string()
            };

            let mut effective = if args.rebase {
                "rebase".to_string()
            } else if args.merge {
                "merge".to_string()
            } else if args.checkout {
                "checkout".to_string()
            } else {
                update_strategy.clone()
            };

            if needs_clone && (effective == "rebase" || effective == "merge") {
                effective = "checkout".to_string();
            }

            if !sub_path.exists() {
                bail!("failed to checkout submodule commit: No such file or directory (os error 2)");
            }

            let checkout_force = args.force || needs_clone;

            if !args.remote
                && !needs_clone
                && read_submodule_head(&sub_path).as_deref() == Some(recorded_oid)
            {
                refresh_gitlink_index_stat(&repo, &m.path)?;
                if let Ok(sub_repo) = Repository::open(&modules_dir, Some(&sub_path)) {
                    let _ = reapply_sparse_checkout_if_configured(&sub_repo);
                } else if let Ok(sub_repo) = Repository::discover(Some(&sub_path)) {
                    let _ = reapply_sparse_checkout_if_configured(&sub_repo);
                }
                if sub_path.join(".git").is_dir() && modules_dir.join("HEAD").exists() {
                    fs::remove_dir_all(sub_path.join(".git")).with_context(|| {
                        format!(
                            "failed to normalize submodule .git after checkout for {}",
                            sub_path.display()
                        )
                    })?;
                    attach_existing_submodule_worktree(&grit_bin, &modules_dir, &sub_path)?;
                }
                continue;
            }

            match effective.as_str() {
                "rebase" => {
                    let mut pull_cmd = grit_subprocess(&grit_bin);
                    pull_cmd
                        .args(["pull", "--rebase", ".", &checkout_oid])
                        .current_dir(&sub_path)
                        .env_remove("GIT_DIR")
                        .env_remove("GIT_WORK_TREE");
                    if args.quiet {
                        pull_cmd.args(["--quiet"]);
                    }
                    let st = pull_cmd.status().context("submodule pull --rebase")?;
                    if !st.success() {
                        bail!("submodule update failed for '{}'", m.path);
                    }
                }
                "merge" => {
                    let mut pull_cmd = grit_subprocess(&grit_bin);
                    pull_cmd
                        .args(["pull", ".", &checkout_oid])
                        .current_dir(&sub_path)
                        .env_remove("GIT_DIR")
                        .env_remove("GIT_WORK_TREE");
                    if args.quiet {
                        pull_cmd.args(["--quiet"]);
                    }
                    let st = pull_cmd.status().context("submodule pull merge")?;
                    if !st.success() {
                        bail!("submodule update failed for '{}'", m.path);
                    }
                }
                _ => {
                    let mut co_cmd = Command::new(&grit_bin);
                    grit_exe::strip_trace2_env(&mut co_cmd);
                    if modules_dir.join("HEAD").exists() {
                        co_cmd
                            .env("GIT_DIR", &modules_dir)
                            .env("GIT_WORK_TREE", &sub_path);
                    }
                    co_cmd.current_dir(&sub_path);
                    co_cmd.arg("checkout");
                    if checkout_force {
                        co_cmd.arg("--force");
                    }
                    co_cmd.arg("--quiet");
                    co_cmd.arg(&checkout_oid);
                    let co_out = co_cmd.output().context("submodule checkout")?;
                    if !co_out.status.success() {
                        let stderr = String::from_utf8_lossy(&co_out.stderr);
                        if !stderr.is_empty() {
                            eprint!("{stderr}");
                        }
                        eprintln!(
                            "fatal: Unable to checkout '{}' in submodule path '{}'",
                            checkout_oid, msg_path
                        );
                        if !args.paths.is_empty() {
                            std::process::exit(1);
                        }
                        any_failure = true;
                        continue;
                    }

                    let mut reset_cmd = Command::new(&grit_bin);
                    reset_cmd
                        .args(["reset", "--hard", "--quiet"])
                        .current_dir(&sub_path);
                    let reset_status = reset_cmd.status().context("reset submodule after checkout")?;
                    if !reset_status.success() {
                        bail!("failed to reset submodule '{}' after checkout", m.name);
                    }
                }
            }

            refresh_gitlink_index_stat(&repo, &m.path)?;

            if args.remote {
                stage_gitlink_in_super_index(&repo, &m.path, &checkout_oid)?;
            }

            if matches!(effective.as_str(), "" | "checkout") || args.remote {
                eprintln!(
                    "Submodule path '{}': checked out '{}'",
                    msg_path, checkout_oid
                );
            }

            if let Ok(sub_repo) = Repository::open(&modules_dir, Some(&sub_path)) {
                let _ = reapply_sparse_checkout_if_configured(&sub_repo);
            } else if let Ok(sub_repo) = Repository::discover(Some(&sub_path)) {
                let _ = reapply_sparse_checkout_if_configured(&sub_repo);
            }

            if sub_path.join(".git").is_dir() && modules_dir.join("HEAD").exists() {
                fs::remove_dir_all(sub_path.join(".git")).with_context(|| {
                    format!(
                        "failed to normalize submodule .git after checkout for {}",
                        sub_path.display()
                    )
                })?;
                attach_existing_submodule_worktree(&grit_bin, &modules_dir, &sub_path)?;
            }
        }
    }

    if any_failure {
        std::process::exit(1);
    }

    let recurse_nested = args.recursive || args.implicit_recursive;
    if recurse_nested {
        for m in &selected {
            let sub_path = work_tree.join(&m.path);
            if !sub_path.join(".git").exists() {
                continue;
            }
            if parse_gitmodules(&sub_path).unwrap_or_default().is_empty() {
                continue;
            }
            let nested = UpdateArgs {
                paths: vec![],
                quiet: args.quiet,
                init: args.init,
                checkout: args.checkout,
                remote: args.remote,
                rebase: args.rebase,
                merge: args.merge,
                force: args.force,
                depth: args.depth,
                jobs: args.jobs,
                filter: args.filter.clone(),
                recursive: recurse_nested,
                implicit_recursive: args.implicit_recursive,
                reference: args.reference.clone(),
                no_recommend_shallow: args.no_recommend_shallow,
            };
            run_update_inner(&nested, Some(sub_path.clone()))?;
        }
    }

    Ok(())
}
