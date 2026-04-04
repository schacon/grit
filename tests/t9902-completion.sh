#!/bin/sh
#
# Upstream: t9902-completion.sh
# Requires bash completion — ported as test_expect_failure stubs.
#

test_description='test bash completion'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- bash completion not available in grit ---

test_expect_failure 'setup for __git_find_repo_path/__gitdir tests' '
	false
'

test_expect_failure '__git_find_repo_path - from command line (through $__git_dir)' '
	false
'

test_expect_failure '__git_find_repo_path - .git directory in cwd' '
	false
'

test_expect_failure '__git_find_repo_path - .git directory in parent' '
	false
'

test_expect_failure '__git_find_repo_path - cwd is a .git directory' '
	false
'

test_expect_failure '__git_find_repo_path - parent is a .git directory' '
	false
'

test_expect_failure '__git_find_repo_path - $GIT_DIR set while .git directory in cwd' '
	false
'

test_expect_failure '__git_find_repo_path - $GIT_DIR set while .git directory in parent' '
	false
'

test_expect_failure '__git_find_repo_path - from command line while "git -C"' '
	false
'

test_expect_failure '__git_find_repo_path - relative dir from command line and "git -C"' '
	false
'

test_expect_failure '__git_find_repo_path - $GIT_DIR set while "git -C"' '
	false
'

test_expect_failure '__git_find_repo_path - relative dir in $GIT_DIR and "git -C"' '
	false
'

test_expect_failure '__git_find_repo_path - "git -C" while .git directory in cwd' '
	false
'

test_expect_failure '__git_find_repo_path - "git -C" while cwd is a .git directory' '
	false
'

test_expect_failure '__git_find_repo_path - "git -C" while .git directory in parent' '
	false
'

test_expect_failure '__git_find_repo_path - non-existing path in "git -C"' '
	false
'

test_expect_failure '__git_find_repo_path - non-existing path in $__git_dir' '
	false
'

test_expect_failure '__git_find_repo_path - non-existing $GIT_DIR' '
	false
'

test_expect_failure '__git_find_repo_path - gitfile in cwd' '
	false
'

test_expect_failure '__git_find_repo_path - gitfile in parent' '
	false
'

test_expect_failure '__git_find_repo_path - resulting path avoids symlinks' '
	false
'

test_expect_failure '__git_find_repo_path - not a git repository' '
	false
'

test_expect_failure '__gitdir - finds repo' '
	false
'

test_expect_failure '__gitdir - returns error when cannot find repo' '
	false
'

test_expect_failure '__gitdir - repo as argument' '
	false
'

test_expect_failure '__gitdir - remote as argument' '
	false
'

test_expect_failure '__git_dequote - plain unquoted word' '
	false
'

test_expect_failure '__git_dequote - backslash escaped' '
	false
'

test_expect_failure '__git_dequote - single quoted' '
	false
'

test_expect_failure '__git_dequote - double quoted' '
	false
'

test_expect_failure '__git_dequote - open single quote' '
	false
'

test_expect_failure '__git_dequote - open double quote' '
	false
'

test_expect_failure '__git_count_path_components - no slashes' '
	false
'

test_expect_failure '__git_count_path_components - relative' '
	false
'

test_expect_failure '__git_count_path_components - absolute' '
	false
'

test_expect_failure '__git_count_path_components - trailing slash' '
	false
'

test_expect_failure '__gitcomp_direct - puts everything into COMPREPLY as-is' '
	false
'

test_expect_failure '__gitcomp - trailing space - options' '
	false
'

test_expect_failure '__gitcomp - trailing space - config keys' '
	false
'

test_expect_failure '__gitcomp - option parameter' '
	false
'

test_expect_failure '__gitcomp - prefix' '
	false
'

test_expect_failure '__gitcomp - suffix' '
	false
'

test_expect_failure '__gitcomp - ignore optional negative options' '
	false
'

test_expect_failure '__gitcomp - ignore/narrow optional negative options' '
	false
'

test_expect_failure '__gitcomp - ignore/narrow optional negative options' '
	false
'

test_expect_failure '__gitcomp - expand all negative options' '
	false
'

test_expect_failure '__gitcomp - expand/narrow all negative options' '
	false
'

test_expect_failure '__gitcomp - equal skip' '
	false
'

test_expect_failure '__gitcomp - doesnt fail because of invalid variable name' '
	false
'

test_expect_failure '__gitcomp_nl - trailing space' '
	false
'

test_expect_failure '__gitcomp_nl - prefix' '
	false
'

test_expect_failure '__gitcomp_nl - suffix' '
	false
'

test_expect_failure '__gitcomp_nl - no suffix' '
	false
'

test_expect_failure '__gitcomp_nl - doesnt fail because of invalid variable name' '
	false
'

test_expect_failure '__git_remotes - list remotes from $GIT_DIR/remotes and from config file' '
	false
'

test_expect_failure '__git_is_configured_remote' '
	false
'

test_expect_failure 'setup for ref completion' '
	false
'

test_expect_failure '__git_refs - simple' '
	false
'

test_expect_failure '__git_refs - full refs' '
	false
'

test_expect_failure '__git_refs - repo given on the command line' '
	false
'

test_expect_failure '__git_refs - remote on local file system' '
	false
'

test_expect_failure '__git_refs - remote on local file system - full refs' '
	false
'

test_expect_failure '__git_refs - configured remote' '
	false
'

test_expect_failure '__git_refs - configured remote - with slash' '
	false
'

test_expect_failure '__git_refs - configured remote - full refs' '
	false
'

test_expect_failure '__git_refs - configured remote - repo given on the command line' '
	false
'

test_expect_failure '__git_refs - configured remote - full refs - repo given on the command line' '
	false
'

test_expect_failure '__git_refs - configured remote - remote name matches a directory' '
	false
'

test_expect_failure '__git_refs - URL remote' '
	false
'

test_expect_failure '__git_refs - URL remote - full refs' '
	false
'

test_expect_failure '__git_refs - non-existing remote' '
	false
'

test_expect_failure '__git_refs - non-existing remote - full refs' '
	false
'

test_expect_failure '__git_refs - non-existing URL remote' '
	false
'

test_expect_failure '__git_refs - non-existing URL remote - full refs' '
	false
'

test_expect_failure '__git_refs - not in a git repository' '
	false
'

test_expect_failure '__git_refs - unique remote branches for git checkout DWIMery' '
	false
'

test_expect_failure '__git_refs - after --opt=' '
	false
'

test_expect_failure '__git_refs - after --opt= - full refs' '
	false
'

test_expect_failure '__git refs - excluding refs' '
	false
'

test_expect_failure '__git refs - excluding full refs' '
	false
'

test_expect_failure 'setup for filtering matching refs' '
	false
'

test_expect_failure '__git_refs - do not filter refs unless told so' '
	false
'

test_expect_failure '__git_refs - only matching refs' '
	false
'

test_expect_failure '__git_refs - only matching refs - full refs' '
	false
'

test_expect_failure '__git_refs - only matching refs - remote on local file system' '
	false
'

test_expect_failure '__git_refs - only matching refs - configured remote' '
	false
'

test_expect_failure '__git_refs - only matching refs - remote - full refs' '
	false
'

test_expect_failure '__git_refs - only matching refs - checkout DWIMery' '
	false
'

test_expect_failure 'teardown after filtering matching refs' '
	false
'

test_expect_failure '__git_refs - for-each-ref format specifiers in prefix' '
	false
'

test_expect_failure '__git_complete_refs - simple' '
	false
'

test_expect_failure '__git_complete_refs - matching' '
	false
'

test_expect_failure '__git_complete_refs - remote' '
	false
'

test_expect_failure '__git_complete_refs - remote - with slash' '
	false
'

test_expect_failure '__git_complete_refs - track' '
	false
'

test_expect_failure '__git_complete_refs - current word' '
	false
'

test_expect_failure '__git_complete_refs - prefix' '
	false
'

test_expect_failure '__git_complete_refs - suffix' '
	false
'

test_expect_failure '__git_complete_fetch_refspecs - simple' '
	false
'

test_expect_failure '__git_complete_fetch_refspecs - with slash' '
	false
'

test_expect_failure '__git_complete_fetch_refspecs - matching' '
	false
'

test_expect_failure '__git_complete_fetch_refspecs - prefix' '
	false
'

test_expect_failure '__git_complete_fetch_refspecs - fully qualified' '
	false
'

test_expect_failure '__git_complete_fetch_refspecs - fully qualified & prefix' '
	false
'

test_expect_failure '__git_complete_worktree_paths' '
	false
'

test_expect_failure '__git_complete_worktree_paths - not a git repository' '
	false
'

test_expect_failure '__git_complete_worktree_paths with -C' '
	false
'

test_expect_failure 'git switch - with no options, complete local branches and unique remote branch names for DWIM logic' '
	false
'

test_expect_failure 'git bisect - when not bisecting, complete only replay and start subcommands' '
	false
'

test_expect_failure 'git bisect - complete options to start subcommand' '
	false
'

test_expect_failure 'setup for git-bisect tests requiring a repo' '
	false
'

test_expect_failure 'git bisect - start subcommand arguments before double-dash are completed as revs' '
	false
'

test_expect_failure 'git bisect - start subcommand arguments after double-dash are not completed' '
	false
'

test_expect_failure 'setup for git-bisect tests requiring ongoing bisection' '
	false
'

test_expect_failure 'git-bisect - when bisecting all subcommands are candidates' '
	false
'

test_expect_failure 'git-bisect - options to terms subcommand are candidates' '
	false
'

test_expect_failure 'git-bisect - git-log options to visualize subcommand are candidates' '
	false
'

test_expect_failure 'git-bisect - view subcommand is not a candidate' '
	false
'

test_expect_failure 'git-bisect - existing view subcommand is recognized and enables completion of git-log options' '
	false
'

test_expect_failure 'git checkout - completes refs and unique remote branches for DWIM' '
	false
'

test_expect_failure 'git switch - with --no-guess, complete only local branches' '
	false
'

test_expect_failure 'git switch - with GIT_COMPLETION_CHECKOUT_NO_GUESS=1, complete only local branches' '
	false
'

test_expect_failure 'git switch - --guess overrides GIT_COMPLETION_CHECKOUT_NO_GUESS=1, complete local branches and unique remote names for DWIM logic' '
	false
'

test_expect_failure 'git switch - a later --guess overrides previous --no-guess, complete local and remote unique branches for DWIM' '
	false
'

test_expect_failure 'git switch - a later --no-guess overrides previous --guess, complete only local branches' '
	false
'

test_expect_failure 'git checkout - with GIT_COMPLETION_NO_GUESS=1 only completes refs' '
	false
'

test_expect_failure 'git checkout - --guess overrides GIT_COMPLETION_NO_GUESS=1, complete refs and unique remote branches for DWIM' '
	false
'

test_expect_failure 'git checkout - with --no-guess, only completes refs' '
	false
'

test_expect_failure 'git checkout - a later --guess overrides previous --no-guess, complete refs and unique remote branches for DWIM' '
	false
'

test_expect_failure 'git checkout - a later --no-guess overrides previous --guess, complete only refs' '
	false
'

test_expect_failure 'git checkout - with checkout.guess = false, only completes refs' '
	false
'

test_expect_failure 'git checkout - with checkout.guess = true, completes refs and unique remote branches for DWIM' '
	false
'

test_expect_failure 'git checkout - a later --guess overrides previous checkout.guess = false, complete refs and unique remote branches for DWIM' '
	false
'

test_expect_failure 'git checkout - a later --no-guess overrides previous checkout.guess = true, complete only refs' '
	false
'

test_expect_failure 'git switch - with --detach, complete all references' '
	false
'

test_expect_failure 'git checkout - with --detach, complete only references' '
	false
'

test_expect_failure 'setup sparse-checkout tests' '
	false
'

test_expect_failure 'sparse-checkout completes subcommands' '
	false
'

test_expect_failure 'cone mode sparse-checkout completes directory names' '
	false
'

test_expect_failure 'cone mode sparse-checkout completes directory names with spaces and accents' '
	false
'

test_expect_failure 'cone mode sparse-checkout completes directory names with tabs' '
	false
'

test_expect_failure 'cone mode sparse-checkout completes directory names with backslashes' '
	false
'

test_expect_failure 'non-cone mode sparse-checkout gives rooted paths' '
	false
'

test_expect_failure 'git sparse-checkout set --cone completes directory names' '
	false
'

test_expect_failure 'git switch - with -d, complete all references' '
	false
'

test_expect_failure 'git checkout - with -d, complete only references' '
	false
'

test_expect_failure 'git switch - with --track, complete only remote branches' '
	false
'

test_expect_failure 'git checkout - with --track, complete only remote branches' '
	false
'

test_expect_failure 'git switch - with --no-track, complete only local branch names' '
	false
'

test_expect_failure 'git checkout - with --no-track, complete only local references' '
	false
'

test_expect_failure 'git switch - with -c, complete all references' '
	false
'

test_expect_failure 'git switch - with -C, complete all references' '
	false
'

test_expect_failure 'git switch - with -c and --track, complete all references' '
	false
'

test_expect_failure 'git switch - with -C and --track, complete all references' '
	false
'

test_expect_failure 'git switch - with -c and --no-track, complete all references' '
	false
'

test_expect_failure 'git switch - with -C and --no-track, complete all references' '
	false
'

test_expect_failure 'git checkout - with -b, complete all references' '
	false
'

test_expect_failure 'git checkout - with -B, complete all references' '
	false
'

test_expect_failure 'git checkout - with -b and --track, complete all references' '
	false
'

test_expect_failure 'git checkout - with -B and --track, complete all references' '
	false
'

test_expect_failure 'git checkout - with -b and --no-track, complete all references' '
	false
'

test_expect_failure 'git checkout - with -B and --no-track, complete all references' '
	false
'

test_expect_failure 'git switch - for -c, complete local branches and unique remote branches' '
	false
'

test_expect_failure 'git switch - for -C, complete local branches and unique remote branches' '
	false
'

test_expect_failure 'git switch - for -c with --no-guess, complete local branches only' '
	false
'

test_expect_failure 'git switch - for -C with --no-guess, complete local branches only' '
	false
'

test_expect_failure 'git switch - for -c with --no-track, complete local branches only' '
	false
'

test_expect_failure 'git switch - for -C with --no-track, complete local branches only' '
	false
'

test_expect_failure 'git checkout - for -b, complete local branches and unique remote branches' '
	false
'

test_expect_failure 'git checkout - for -B, complete local branches and unique remote branches' '
	false
'

test_expect_failure 'git checkout - for -b with --no-guess, complete local branches only' '
	false
'

test_expect_failure 'git checkout - for -B with --no-guess, complete local branches only' '
	false
'

test_expect_failure 'git checkout - for -b with --no-track, complete local branches only' '
	false
'

test_expect_failure 'git checkout - for -B with --no-track, complete local branches only' '
	false
'

test_expect_failure 'git switch - with --orphan completes local branch names and unique remote branch names' '
	false
'

test_expect_failure 'git switch - --orphan with branch already provided completes nothing else' '
	false
'

test_expect_failure 'git checkout - with --orphan completes local branch names and unique remote branch names' '
	false
'

test_expect_failure 'git checkout - --orphan with branch already provided completes local refs for a start-point' '
	false
'

test_expect_failure 'git restore completes modified files' '
	false
'

test_expect_failure 'teardown after ref completion' '
	false
'

test_expect_failure 'setup for path completion tests' '
	false
'

test_expect_failure '__git_complete_index_file - simple' '
	false
'

test_expect_failure '__git_complete_index_file - UTF-8 in ls-files output' '
	false
'

test_expect_failure 'teardown after path completion tests' '
	false
'

test_expect_failure '__git_find_on_cmdline - single match' '
	false
'

test_expect_failure '__git_find_on_cmdline - multiple matches' '
	false
'

test_expect_failure '__git_find_on_cmdline - no match' '
	false
'

test_expect_failure '__git_find_on_cmdline - single match with index' '
	false
'

test_expect_failure '__git_find_on_cmdline - multiple matches with index' '
	false
'

test_expect_failure '__git_find_on_cmdline - no match with index' '
	false
'

test_expect_failure '__git_find_on_cmdline - ignores matches before command with index' '
	false
'

test_expect_failure '__git_get_config_variables' '
	false
'

test_expect_failure '__git_pretty_aliases' '
	false
'

test_expect_failure 'basic' '
	false
'

test_expect_failure 'double dash "git" itself' '
	false
'

test_expect_failure 'double dash "git checkout"' '
	false
'

test_expect_failure 'general options' '
	false
'

test_expect_failure 'general options plus command' '
	false
'

test_expect_failure 'git --help completion' '
	false
'

test_expect_failure 'completion.commands removes multiple commands' '
	false
'

test_expect_failure 'setup for integration tests' '
	false
'

test_expect_failure 'checkout completes ref names' '
	false
'

test_expect_failure 'checkout does not match ref names of a different case' '
	false
'

test_expect_failure 'checkout matches case insensitively with GIT_COMPLETION_IGNORE_CASE' '
	false
'

test_expect_failure 'checkout completes pseudo refs' '
	false
'

test_expect_failure 'checkout completes pseudo refs case insensitively with GIT_COMPLETION_IGNORE_CASE' '
	false
'

test_expect_failure 'git -C <path> checkout uses the right repo' '
	false
'

test_expect_failure 'show completes all refs' '
	false
'

test_expect_failure '<ref>: completes paths' '
	false
'

test_expect_failure 'complete tree filename with spaces' '
	false
'

test_expect_failure 'complete tree filename with metacharacters' '
	false
'

test_expect_failure 'symbolic-ref completes builtin options' '
	false
'

test_expect_failure 'symbolic-ref completes short ref names' '
	false
'

test_expect_failure 'symbolic-ref completes full ref names' '
	false
'

test_expect_failure 'send-email' '
	false
'

test_expect_failure 'complete files' '
	false
'

test_expect_failure 'completion uses <cmd> completion for alias: !f () { VAR=val git <cmd> ... }' '
	false
'

test_expect_failure 'completion used <cmd> completion for alias: !f() { : git <cmd> ; ... }' '
	false
'

test_expect_failure 'completion used <cmd> completion for alias: !f() { : <cmd> ; ... }' '
	false
'

test_expect_failure 'completion used <cmd> completion for alias: !f() { : <cmd>; ... }' '
	false
'

test_expect_failure 'completion without explicit _git_xxx function' '
	false
'

test_expect_failure 'complete with tilde expansion' '
	false
'

test_expect_failure 'setup other remote for remote reference completion' '
	false
'

test_expect_failure 'git config subcommand' '
	false
'

test_expect_failure 'git config subcommand options' '
	false
'

test_expect_failure 'git config get' '
	false
'

test_expect_failure 'git config set - section' '
	false
'

test_expect_failure 'git config set - section include, includeIf' '
	false
'

test_expect_failure 'git config set - variable name' '
	false
'

test_expect_failure 'git config set - variable name include' '
	false
'

test_expect_failure 'setup for git config submodule tests' '
	false
'

test_expect_failure 'git config set - variable name - submodule and __git_compute_first_level_config_vars_for_section' '
	false
'

test_expect_failure 'git config set - variable name - __git_compute_second_level_config_vars_for_section' '
	false
'

test_expect_failure 'git config set - value' '
	false
'

test_expect_failure 'git -c - section' '
	false
'

test_expect_failure 'git -c - variable name' '
	false
'

test_expect_failure 'git -c - value' '
	false
'

test_expect_failure 'git clone --config= - section' '
	false
'

test_expect_failure 'git clone --config= - variable name' '
	false
'

test_expect_failure 'git clone --config= - value' '
	false
'

test_expect_failure 'git reflog show' '
	false
'

test_expect_failure 'options with value' '
	false
'

test_expect_failure 'sourcing the completion script clears cached commands' '
	false
'

test_expect_failure 'sourcing the completion script clears cached merge strategies' '
	false
'

test_expect_failure 'sourcing the completion script clears cached --options' '
	false
'

test_expect_failure 'option aliases are not shown by default' '
	false
'

test_expect_failure 'option aliases are shown with GIT_COMPLETION_SHOW_ALL' '
	false
'

test_expect_failure 'plumbing commands are excluded without GIT_COMPLETION_SHOW_ALL_COMMANDS' '
	false
'

test_expect_failure 'all commands are shown with GIT_COMPLETION_SHOW_ALL_COMMANDS (also main non-builtin)' '
	false
'

test_expect_failure '__git_complete' '
	false
'

test_expect_failure '__git_pseudoref_exists' '
	false
'

test_expect_failure 'simple alias' '
	false
'

test_expect_failure 'recursive alias' '
	false
'

test_expect_failure 'completion uses <cmd> completion for alias: !sh -c '\''git <cmd> ...'\''' '
	false
'

test_expect_failure '__git_complete_remote_or_refspec - push $flag other' '
	false
'

test_expect_failure '__git_complete_remote_or_refspec - push other $flag' '
	false
'

test_expect_failure '__git_complete_index_file - escaped characters on cmdline' '
	false
'

test_expect_failure '__git_complete_index_file - quoted characters on cmdline' '
	false
'

test_done
