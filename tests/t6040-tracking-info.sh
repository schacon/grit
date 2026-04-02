#!/bin/sh
#
# Tests for tracking info — branch remote config, status with tracking.
# Ported subset from git/t/t6040-tracking-info.sh (upstream ~44 tests).
# grit has limited tracking display; we verify config, branch listing,
# status -sb, show-ref, and ls-remote with remote tracking refs.
# We use /usr/bin/git for remote/push/fetch setup (not implemented in grit).

test_description='grit branch tracking configuration and info'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT=/usr/bin/git

# ---------------------------------------------------------------------------
# Setup — origin bare repo + local with tracking
# ---------------------------------------------------------------------------
test_expect_success 'setup origin and local repo with tracking' '
	git init --bare origin.git &&
	git init repo &&
	cd repo &&
	git config user.name "Test User" &&
	git config user.email "test@example.com" &&
	echo "content" >file &&
	git add file &&
	git commit -m "initial" &&
	$REAL_GIT remote add origin ../origin.git &&
	$REAL_GIT push -u origin master
'

# ---------------------------------------------------------------------------
# Config-level tracking
# ---------------------------------------------------------------------------
test_expect_success 'config shows correct remote for master' '
	cd repo &&
	remote=$(git config branch.master.remote) &&
	test "$remote" = "origin"
'

test_expect_success 'config shows correct merge ref for master' '
	cd repo &&
	merge=$(git config branch.master.merge) &&
	test "$merge" = "refs/heads/master"
'

# ---------------------------------------------------------------------------
# Branch -v / -vv
# ---------------------------------------------------------------------------
test_expect_success 'branch -v shows commit info' '
	cd repo &&
	git branch -v >out &&
	grep "master" out &&
	grep "initial" out
'

test_expect_success 'branch -vv shows branch info' '
	cd repo &&
	git branch -vv >out &&
	grep "master" out
'

# ---------------------------------------------------------------------------
# Status -sb shows branch
# ---------------------------------------------------------------------------
test_expect_success 'status -sb shows branch name on master' '
	cd repo &&
	git status -sb >out &&
	grep "^## master" out
'

# ---------------------------------------------------------------------------
# Feature branch with tracking
# ---------------------------------------------------------------------------
test_expect_success 'create and push feature branch' '
	cd repo &&
	git checkout -b feature &&
	echo "feature" >feature.txt &&
	git add feature.txt &&
	git commit -m "feature commit" &&
	$REAL_GIT push -u origin feature
'

test_expect_success 'feature branch tracking config is set' '
	cd repo &&
	test "$(git config branch.feature.remote)" = "origin" &&
	test "$(git config branch.feature.merge)" = "refs/heads/feature"
'

test_expect_success 'status -sb on feature shows branch' '
	cd repo &&
	git status -sb >out &&
	grep "^## feature" out
'

# ---------------------------------------------------------------------------
# Branch listing with remotes
# ---------------------------------------------------------------------------
test_expect_success 'branch -r lists remote branches' '
	cd repo &&
	git branch -r >out &&
	grep "origin/master" out &&
	grep "origin/feature" out
'

test_expect_success 'branch -a lists all branches' '
	cd repo &&
	git branch -a >out &&
	grep "master" out &&
	grep "feature" out &&
	grep "origin/master" out
'

# ---------------------------------------------------------------------------
# Local-only branch (no tracking)
# ---------------------------------------------------------------------------
test_expect_success 'create local-only branch' '
	cd repo &&
	git checkout master &&
	git checkout -b local-only &&
	echo "local" >local.txt &&
	git add local.txt &&
	git commit -m "local only"
'

test_expect_success 'local-only branch has no remote config' '
	cd repo &&
	test_must_fail git config branch.local-only.remote
'

test_expect_success 'local-only branch has no merge config' '
	cd repo &&
	test_must_fail git config branch.local-only.merge
'

# ---------------------------------------------------------------------------
# Show-ref with remote tracking
# ---------------------------------------------------------------------------
test_expect_success 'show-ref lists remote tracking refs' '
	cd repo &&
	git show-ref >out &&
	grep "refs/remotes/origin/master" out
'

test_expect_success 'show-ref lists local branch refs' '
	cd repo &&
	git show-ref >out &&
	grep "refs/heads/master" out &&
	grep "refs/heads/feature" out
'

# ---------------------------------------------------------------------------
# ls-remote
# ---------------------------------------------------------------------------
test_expect_success 'ls-remote with path shows refs' '
	cd repo &&
	git ls-remote ../origin.git >out &&
	grep "refs/heads/master" out &&
	grep "refs/heads/feature" out
'

# ---------------------------------------------------------------------------
# Status porcelain -b
# ---------------------------------------------------------------------------
test_expect_success 'status --porcelain -b shows branch header' '
	cd repo &&
	git checkout master &&
	git status --porcelain -b >out &&
	grep "^## master" out
'

# ---------------------------------------------------------------------------
# Config manipulation
# ---------------------------------------------------------------------------
test_expect_success 'change tracking remote via config' '
	cd repo &&
	git config branch.master.remote upstream &&
	test "$(git config branch.master.remote)" = "upstream" &&
	git config branch.master.remote origin
'

test_expect_success 'config --unset removes tracking' '
	cd repo &&
	git config branch.local-only.remote origin &&
	git config --unset branch.local-only.remote &&
	test_must_fail git config branch.local-only.remote
'

# ---------------------------------------------------------------------------
# Multiple branches, verify branch listing
# ---------------------------------------------------------------------------
test_expect_success 'branch -l lists local branches' '
	cd repo &&
	git branch -l >out &&
	grep "master" out &&
	grep "feature" out &&
	grep "local-only" out
'

test_expect_success 'branch --contains HEAD shows current branch' '
	cd repo &&
	git checkout master &&
	git branch --contains HEAD >out &&
	grep "master" out
'

test_done
