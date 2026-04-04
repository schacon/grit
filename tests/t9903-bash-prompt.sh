#!/bin/sh
#
# Upstream: t9903-bash-prompt.sh
# Requires bash prompt functions — ported as test_expect_failure stubs.
#

test_description='test git-specific bash prompt functions'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- bash prompt functions not available in grit ---

test_expect_failure 'setup for prompt tests' '
	false
'

test_expect_failure 'prompt - branch name' '
	false
'

test_expect_failure 'prompt - branch name - symlink symref' '
	false
'

test_expect_failure 'prompt - unborn branch' '
	false
'

test_expect_failure 'prompt - with newline in path' '
	false
'

test_expect_failure 'prompt - detached head' '
	false
'

test_expect_failure 'prompt - describe detached head - contains' '
	false
'

test_expect_failure 'prompt - describe detached head - branch' '
	false
'

test_expect_failure 'prompt - describe detached head - describe' '
	false
'

test_expect_failure 'prompt - describe detached head - default' '
	false
'

test_expect_failure 'prompt - inside .git directory' '
	false
'

test_expect_failure 'prompt - deep inside .git directory' '
	false
'

test_expect_failure 'prompt - inside bare repository' '
	false
'

test_expect_failure 'prompt - interactive rebase' '
	false
'

test_expect_failure 'prompt - rebase merge' '
	false
'

test_expect_failure 'prompt - rebase am' '
	false
'

test_expect_failure 'prompt - merge' '
	false
'

test_expect_failure 'prompt - cherry-pick' '
	false
'

test_expect_failure 'prompt - revert' '
	false
'

test_expect_failure 'prompt - bisect' '
	false
'

test_expect_failure 'prompt - dirty status indicator - clean' '
	false
'

test_expect_failure 'prompt - dirty status indicator - dirty worktree' '
	false
'

test_expect_failure 'prompt - dirty status indicator - dirty index' '
	false
'

test_expect_failure 'prompt - dirty status indicator - dirty index and worktree' '
	false
'

test_expect_failure 'prompt - dirty status indicator - orphan branch - clean' '
	false
'

test_expect_failure 'prompt - dirty status indicator - orphan branch - dirty index' '
	false
'

test_expect_failure 'prompt - dirty status indicator - orphan branch - dirty index and worktree' '
	false
'

test_expect_failure 'prompt - dirty status indicator - shell variable unset with config disabled' '
	false
'

test_expect_failure 'prompt - dirty status indicator - shell variable unset with config enabled' '
	false
'

test_expect_failure 'prompt - dirty status indicator - shell variable set with config disabled' '
	false
'

test_expect_failure 'prompt - dirty status indicator - shell variable set with config enabled' '
	false
'

test_expect_failure 'prompt - dirty status indicator - not shown inside .git directory' '
	false
'

test_expect_failure 'prompt - stash status indicator - no stash' '
	false
'

test_expect_failure 'prompt - stash status indicator - stash' '
	false
'

test_expect_failure 'prompt - stash status indicator - not shown inside .git directory' '
	false
'

test_expect_failure 'prompt - untracked files status indicator - no untracked files' '
	false
'

test_expect_failure 'prompt - untracked files status indicator - untracked files' '
	false
'

test_expect_failure 'prompt - untracked files status indicator - empty untracked dir' '
	false
'

test_expect_failure 'prompt - untracked files status indicator - non-empty untracked dir' '
	false
'

test_expect_failure 'prompt - untracked files status indicator - untracked files outside cwd' '
	false
'

test_expect_failure 'prompt - untracked files status indicator - shell variable unset with config disabled' '
	false
'

test_expect_failure 'prompt - untracked files status indicator - shell variable unset with config enabled' '
	false
'

test_expect_failure 'prompt - untracked files status indicator - shell variable set with config disabled' '
	false
'

test_expect_failure 'prompt - untracked files status indicator - shell variable set with config enabled' '
	false
'

test_expect_failure 'prompt - untracked files status indicator - not shown inside .git directory' '
	false
'

test_expect_failure 'prompt - format string starting with dash' '
	false
'

test_expect_failure 'prompt - pc mode' '
	false
'

test_expect_failure 'prompt - bash color pc mode - branch name' '
	false
'

test_expect_failure 'prompt - bash color pc mode - detached head' '
	false
'

test_expect_failure 'prompt - bash color pc mode - dirty status indicator - dirty worktree' '
	false
'

test_expect_failure 'prompt - bash color pc mode - dirty status indicator - dirty index' '
	false
'

test_expect_failure 'prompt - bash color pc mode - dirty status indicator - dirty index and worktree' '
	false
'

test_expect_failure 'prompt - bash color pc mode - dirty status indicator - before root commit' '
	false
'

test_expect_failure 'prompt - bash color pc mode - inside .git directory' '
	false
'

test_expect_failure 'prompt - bash color pc mode - stash status indicator' '
	false
'

test_expect_failure 'prompt - bash color pc mode - untracked files status indicator' '
	false
'

test_expect_failure 'prompt - zsh color pc mode' '
	false
'

test_expect_failure 'prompt - hide if pwd ignored - env var unset, config disabled' '
	false
'

test_expect_failure 'prompt - hide if pwd ignored - env var unset, config disabled, pc mode' '
	false
'

test_expect_failure 'prompt - hide if pwd ignored - env var unset, config unset' '
	false
'

test_expect_failure 'prompt - hide if pwd ignored - env var unset, config unset, pc mode' '
	false
'

test_expect_failure 'prompt - hide if pwd ignored - env var set, config disabled' '
	false
'

test_expect_failure 'prompt - hide if pwd ignored - env var set, config disabled, pc mode' '
	false
'

test_expect_failure 'prompt - hide if pwd ignored - env var set, config unset' '
	false
'

test_expect_failure 'prompt - hide if pwd ignored - env var set, config unset, pc mode' '
	false
'

test_expect_failure 'prompt - hide if pwd ignored - inside gitdir' '
	false
'

test_expect_failure 'prompt - conflict indicator' '
	false
'

test_done
