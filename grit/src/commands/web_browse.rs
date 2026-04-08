//! `git web--browse` — launch a web browser with the given URL(s).
//!
//! Behaviour matches Git’s `git-web--browse.sh`: known browsers invoke an
//! executable (possibly from `browser.<tool>.path`); custom tools use
//! `browser.<tool>.cmd` evaluated by the shell with URL arguments, like Git.

use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use grit_lib::config::ConfigSet;
use grit_lib::repo::Repository;
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

/// Arguments for `git web--browse`.
#[derive(Debug, ClapArgs)]
#[command(about = "Launch a web browser for URL(s)")]
pub struct Args {
    /// Browser tool name (same as `--tool` / `-t`).
    #[arg(short = 'b', long = "browser")]
    pub browser: Option<String>,

    /// Same as `--browser` (Git compatibility).
    #[arg(short = 't', long = "tool")]
    pub tool: Option<String>,

    /// Config variable to read the browser name from (before `web.browser`).
    #[arg(short = 'c', long = "config", value_name = "CONF.VAR")]
    pub config_var: Option<String>,

    /// URL or file to open.
    #[arg(required = true)]
    pub url: Vec<String>,
}

const KNOWN_BROWSERS: &[&str] = &[
    "firefox",
    "iceweasel",
    "seamonkey",
    "iceape",
    "chrome",
    "google-chrome",
    "chromium",
    "chromium-browser",
    "konqueror",
    "opera",
    "w3m",
    "elinks",
    "links",
    "lynx",
    "dillo",
    "open",
    "start",
    "cygstart",
    "xdg-open",
];

/// Run `git web--browse`.
pub fn run(args: Args) -> Result<()> {
    let Args {
        browser,
        tool,
        config_var,
        url: urls,
    } = args;
    if urls.is_empty() {
        bail!("usage: git web--browse [--browser=browser|--tool=browser] [--config=conf.var] url/file ...");
    }

    let browser_choice = browser.or(tool);

    let git_dir = Repository::discover(None).ok().map(|r| r.git_dir);
    let config = ConfigSet::load(git_dir.as_deref(), true).unwrap_or_default();

    let browser = resolve_browser_name(browser_choice.as_deref(), config_var.as_deref(), &config)?;
    let browser_cmd = config_get(&config, &format!("browser.{browser}.cmd"));
    let browser_path_cfg = config_get(&config, &format!("browser.{browser}.path"));

    if !is_known_browser(&browser) && browser_cmd.is_none() {
        bail!("Unknown browser '{browser}'.");
    }

    if browser_cmd.is_none() {
        let path = init_browser_path(&browser, browser_path_cfg.as_deref(), &config);
        if !is_executable_on_path(&path) {
            bail!("The browser {browser} is not available as '{path}'.");
        }
    }

    launch(
        &browser,
        browser_cmd.as_deref(),
        browser_path_cfg.as_deref(),
        &config,
        &urls,
    )
}

fn config_get(config: &ConfigSet, key: &str) -> Option<String> {
    config.get(key).filter(|s| !s.is_empty())
}

fn is_known_browser(name: &str) -> bool {
    KNOWN_BROWSERS.contains(&name)
}

fn valid_custom_tool(config: &ConfigSet, name: &str) -> bool {
    config_get(config, &format!("browser.{name}.cmd")).is_some()
}

fn resolve_browser_name(
    cli_browser: Option<&str>,
    config_var: Option<&str>,
    config: &ConfigSet,
) -> Result<String> {
    if let Some(b) = cli_browser {
        if !is_known_browser(b) && !valid_custom_tool(config, b) {
            bail!("Unknown browser '{b}'.");
        }
        return Ok(b.to_string());
    }

    for opt in [config_var, Some("web.browser")] {
        let Some(key) = opt else {
            continue;
        };
        if key.is_empty() {
            continue;
        }
        if let Some(b) = config_get(config, key) {
            if is_known_browser(&b) || valid_custom_tool(config, &b) {
                return Ok(b);
            }
            eprintln!("git config option {key} set to unknown browser: {b}");
            eprintln!("Resetting to default...");
        }
    }

    pick_default_browser(config)
}

fn pick_default_browser(config: &ConfigSet) -> Result<String> {
    let display_set = std::env::var("DISPLAY")
        .map(|v| !v.is_empty())
        .unwrap_or(false);
    let mut candidates: Vec<&'static str> = if display_set {
        vec![
            "firefox",
            "iceweasel",
            "google-chrome",
            "chrome",
            "chromium",
            "chromium-browser",
            "konqueror",
            "opera",
            "seamonkey",
            "iceape",
            "w3m",
            "elinks",
            "links",
            "lynx",
            "dillo",
            "xdg-open",
        ]
    } else {
        vec!["w3m", "elinks", "links", "lynx"]
    };

    if std::env::var("KDE_FULL_SESSION")
        .map(|v| v == "true")
        .unwrap_or(false)
    {
        candidates.insert(0, "konqueror");
    }

    if std::env::var("SECURITYSESSIONID")
        .map(|v| !v.is_empty())
        .unwrap_or(false)
        || std::env::var("TERM_PROGRAM")
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    {
        candidates.insert(0, "open");
    }

    if Path::new("/bin/start").is_file() {
        candidates.insert(0, "start");
    }
    if Path::new("/usr/bin/cygstart").is_file() {
        candidates.insert(0, "cygstart");
    }

    for name in candidates {
        let path = init_browser_path(name, None, config);
        if is_executable_on_path(&path) {
            return Ok(name.to_string());
        }
    }

    bail!("No known browser available.")
}

fn init_browser_path(browser: &str, configured: Option<&str>, config: &ConfigSet) -> String {
    if let Some(p) = configured.filter(|s| !s.is_empty()) {
        return p.to_string();
    }
    if browser == "chromium" {
        if let Some(p) = config_get(config, "browser.chromium.path") {
            return p;
        }
        if is_executable_on_path("chromium-browser") {
            return "chromium-browser".to_string();
        }
    }
    browser.to_string()
}

fn is_executable_on_path(program: &str) -> bool {
    let path = Path::new(program);
    if program.contains('/') {
        return path.is_file() && is_executable_file(path);
    }
    find_in_path(program)
}

fn find_in_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let full = dir.join(name);
                full.is_file() && is_executable_file(&full)
            })
        })
        .unwrap_or(false)
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
}

fn launch(
    browser: &str,
    browser_cmd: Option<&str>,
    browser_path_cfg: Option<&str>,
    config: &ConfigSet,
    urls: &[String],
) -> Result<()> {
    let browser_path = init_browser_path(browser, browser_path_cfg, config);

    if let Some(cmd) = browser_cmd.filter(|s| !s.is_empty()) {
        return launch_custom_shell(cmd, urls);
    }

    match browser {
        "firefox" | "iceweasel" | "seamonkey" | "iceape" => {
            let newtab = firefox_newtab_flag(&browser_path);
            let mut c = Command::new(&browser_path);
            if let Some(f) = newtab {
                c.arg(f);
            }
            c.args(urls);
            c.stdin(Stdio::null());
            c.stdout(Stdio::inherit());
            c.stderr(Stdio::inherit());
            spawn_detached(&mut c)?;
        }
        "google-chrome" | "chrome" | "chromium" | "chromium-browser" => {
            let mut c = Command::new(&browser_path);
            c.args(urls);
            c.stdin(Stdio::null());
            c.stdout(Stdio::inherit());
            c.stderr(Stdio::inherit());
            spawn_detached(&mut c)?;
        }
        "konqueror" => {
            let base = Path::new(&browser_path)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            let (prog, extra): (String, Vec<OsString>) = match base {
                "konqueror" => {
                    let kfm = replace_trailing_component(&browser_path, "konqueror", "kfmclient");
                    if is_executable_on_path(&kfm) {
                        (kfm, vec![OsString::from("newTab")])
                    } else {
                        (browser_path.clone(), vec![])
                    }
                }
                "kfmclient" => (browser_path.clone(), vec![OsString::from("newTab")]),
                _ => (browser_path.clone(), vec![]),
            };
            let mut c = Command::new(&prog);
            for a in &extra {
                c.arg(a);
            }
            c.args(urls);
            c.stdin(Stdio::null());
            c.stdout(Stdio::inherit());
            c.stderr(Stdio::inherit());
            spawn_detached(&mut c)?;
        }
        "w3m" | "elinks" | "links" | "lynx" | "open" | "cygstart" | "xdg-open" => {
            let status = Command::new(&browser_path)
                .args(urls)
                .status()
                .with_context(|| format!("failed to run {browser_path}"))?;
            if !status.success() {
                bail!("browser exited with status {status}");
            }
        }
        "start" => {
            let status = Command::new(&browser_path)
                .arg("\"web-browse\"")
                .args(urls)
                .status()
                .with_context(|| format!("failed to run {browser_path}"))?;
            if !status.success() {
                bail!("browser exited with status {status}");
            }
        }
        "opera" | "dillo" => {
            let mut c = Command::new(&browser_path);
            c.args(urls);
            c.stdin(Stdio::null());
            c.stdout(Stdio::inherit());
            c.stderr(Stdio::inherit());
            spawn_detached(&mut c)?;
        }
        other => {
            bail!("internal error: unhandled browser '{other}'");
        }
    }
    Ok(())
}

fn replace_trailing_component(path: &str, from: &str, to: &str) -> String {
    let p = Path::new(path);
    if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
        if name == from {
            if let Some(parent) = p.parent().filter(|x| !x.as_os_str().is_empty()) {
                return parent.join(to).to_string_lossy().to_string();
            }
            return to.to_string();
        }
    }
    path.to_string()
}

fn firefox_newtab_flag(browser_path: &str) -> Option<&'static str> {
    let out = Command::new(browser_path).arg("-version").output().ok()?;
    if !out.status.success() {
        return Some("-new-tab");
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let major: u32 = text
        .split_whitespace()
        .filter_map(|w| w.parse::<u32>().ok())
        .next()?;
    if major < 2 {
        None
    } else {
        Some("-new-tab")
    }
}

fn launch_custom_shell(browser_cmd: &str, urls: &[String]) -> Result<()> {
    let status = Command::new("sh")
        .env("BROWSER_CMD", browser_cmd)
        .arg("-c")
        .arg(r#"eval "$BROWSER_CMD \"\$@\"""#)
        .arg("_")
        .args(urls)
        .status()
        .context("failed to run shell for custom browser command")?;
    if !status.success() {
        bail!("browser command exited with status {status}");
    }
    Ok(())
}

fn spawn_detached(cmd: &mut Command) -> Result<()> {
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());
    let _child = cmd.spawn().context("failed to spawn browser")?;
    Ok(())
}
