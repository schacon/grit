/// Returns the path to the current git executable.
pub fn grit_executable() -> std::path::PathBuf {
    std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("git"))
}
