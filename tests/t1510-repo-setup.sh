#!/bin/sh
#
# t1510-repo-setup.sh — GIT_DIR, GIT_WORK_TREE, --git-dir, bare repo detection
#

test_description='repository setup and discovery'
. ./test-lib.sh

# ── basic repo detection ─────────────────────────────────────────────────────

test_expect_success 'setup: init normal repo' '
	git init normal-repo &&
	cd normal-repo &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo content >file &&
	git add file &&
	git commit -m "init"
'

test_expect_success 'rev-parse --is-inside-work-tree in normal repo' '
	cd normal-repo &&
	result=$(git rev-parse --is-inside-work-tree) &&
	test "$result" = "true"
'

test_expect_success 'rev-parse --is-bare-repository in normal repo' '
	cd normal-repo &&
	result=$(git rev-parse --is-bare-repository) &&
	test "$result" = "false"
'

test_expect_success 'rev-parse --git-dir in normal repo' '
	cd normal-repo &&
	result=$(git rev-parse --git-dir) &&
	test "$result" = ".git"
'

test_expect_success 'rev-parse --show-toplevel in normal repo' '
	cd normal-repo &&
	result=$(git rev-parse --show-toplevel) &&
	test "$result" = "$(pwd)"
'

# ── bare repo ────────────────────────────────────────────────────────────────

test_expect_success 'setup: init bare repo' '
	git init --bare bare-repo.git
'

test_expect_success 'rev-parse --is-bare-repository in bare repo' '
	cd bare-repo.git &&
	result=$(git rev-parse --is-bare-repository) &&
	test "$result" = "true"
'

test_expect_success 'rev-parse --git-dir in bare repo is .' '
	cd bare-repo.git &&
	result=$(git rev-parse --git-dir) &&
	test "$result" = "."
'

test_expect_success 'bare repo has no worktree' '
	cd bare-repo.git &&
	result=$(git rev-parse --is-inside-work-tree) &&
	test "$result" = "false"
'

# ── subdirectory discovery ───────────────────────────────────────────────────

test_expect_success 'rev-parse --show-toplevel from subdirectory' '
	cd normal-repo &&
	mkdir -p a/b/c &&
	cd a/b/c &&
	result=$(git rev-parse --show-toplevel) &&
	expected=$(cd ../../.. && pwd) &&
	test "$result" = "$expected"
'

test_expect_success 'rev-parse --is-inside-work-tree from subdirectory' '
	cd normal-repo/a/b/c &&
	result=$(git rev-parse --is-inside-work-tree) &&
	test "$result" = "true"
'

test_expect_success 'rev-parse --git-dir from subdirectory' '
	cd normal-repo/a/b &&
	result=$(git rev-parse --git-dir) &&
	echo "$result" | grep "\.git$"
'

# ── GIT_DIR environment variable ────────────────────────────────────────────

test_expect_success 'GIT_DIR overrides automatic discovery' '
	GIT_DIR="$(pwd)/normal-repo/.git" \
	git rev-parse --git-dir >actual &&
	cat actual &&
	grep "normal-repo/\.git" actual
'

test_expect_success 'GIT_DIR pointing to bare repo works' '
	GIT_DIR="$(pwd)/bare-repo.git" \
	git rev-parse --is-bare-repository >actual &&
	grep "true" actual
'

test_expect_success 'GIT_DIR from outside worktree: is-inside-work-tree is false' '
	cd /tmp &&
	result=$(GIT_DIR="'"$(pwd)"'/normal-repo/.git" git rev-parse --is-inside-work-tree) &&
	test "$result" = "false"
'

# ── GIT_WORK_TREE environment variable ──────────────────────────────────────

test_expect_success 'GIT_WORK_TREE with GIT_DIR shows correct toplevel' '
	GIT_DIR="$(pwd)/normal-repo/.git" \
	GIT_WORK_TREE="$(pwd)/normal-repo" \
	git rev-parse --show-toplevel >actual &&
	grep "normal-repo" actual
'

# ── --git-dir flag ───────────────────────────────────────────────────────────

test_expect_success '--git-dir flag works for rev-parse' '
	git --git-dir="$(pwd)/normal-repo/.git" rev-parse --git-dir >actual &&
	grep "normal-repo/\.git" actual
'

test_expect_success '--git-dir flag works for log' '
	git --git-dir="$(pwd)/normal-repo/.git" log --format="%s" -n 1 >actual &&
	echo "init" >expect &&
	test_cmp expect actual
'

# ── rev-parse --absolute-git-dir ─────────────────────────────────────────────

# NOTE: grit does not yet support --absolute-git-dir
test_expect_failure 'rev-parse --absolute-git-dir returns absolute path' '
	cd normal-repo &&
	result=$(git rev-parse --absolute-git-dir 2>/dev/null) &&
	case "$result" in
	/*) true ;;
	*) echo "not absolute: $result"; false ;;
	esac
'

# ── multiple repos side by side ──────────────────────────────────────────────

test_expect_success 'repos in sibling dirs are independent' '
	git init repo-a &&
	git init repo-b &&
	cd repo-a &&
	git config user.name A && git config user.email a@a &&
	echo a >file && git add file && git commit -m "repo-a init" &&
	cd ../repo-b &&
	git config user.name B && git config user.email b@b &&
	echo b >file && git add file && git commit -m "repo-b init" &&
	cd ../repo-a &&
	git log --format="%s" -n 1 >actual &&
	echo "repo-a init" >expect &&
	test_cmp expect actual &&
	cd ../repo-b &&
	git log --format="%s" -n 1 >actual &&
	echo "repo-b init" >expect &&
	test_cmp expect actual
'

# ── -C flag ──────────────────────────────────────────────────────────────────

test_expect_success '-C flag changes directory before running' '
	git -C normal-repo rev-parse --show-toplevel >actual &&
	grep "normal-repo" actual
'

test_expect_success '-C flag works for log' '
	git -C normal-repo log --format="%s" -n 1 >actual &&
	echo "init" >expect &&
	test_cmp expect actual
'

test_done
