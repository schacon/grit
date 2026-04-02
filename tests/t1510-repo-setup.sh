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

# ── nested bare repos ─────────────────────────────────────────────────────

test_expect_success 'bare repo has refs/ directory' '
	cd bare-repo.git &&
	test -d refs
'

test_expect_success 'bare repo has objects/ directory' '
	cd bare-repo.git &&
	test -d objects
'

test_expect_success 'bare repo has HEAD file' '
	cd bare-repo.git &&
	test -f HEAD
'

# ── rev-parse --show-prefix ───────────────────────────────────────────────

test_expect_success 'rev-parse --show-prefix from root is empty' '
	cd normal-repo &&
	result=$(git rev-parse --show-prefix) &&
	test -z "$result"
'

test_expect_success 'rev-parse --show-prefix from subdir shows path' '
	cd normal-repo/a/b &&
	result=$(git rev-parse --show-prefix) &&
	test "$result" = "a/b/"
'

# ── GIT_DIR with relative path ────────────────────────────────────────────

test_expect_success 'GIT_DIR with relative path works' '
	cd normal-repo &&
	GIT_DIR=.git git rev-parse HEAD >actual &&
	test -s actual
'

# ── init in existing directory ─────────────────────────────────────────────

test_expect_success 'init in existing non-empty directory' '
	mkdir -p existing-dir &&
	echo "existing" >existing-dir/file.txt &&
	git init existing-dir &&
	test -d existing-dir/.git &&
	test -f existing-dir/file.txt
'

test_expect_success 're-init existing repo does not destroy data' '
	git init normal-repo &&
	cd normal-repo &&
	git log --format="%s" -n 1 >actual &&
	echo "init" >expect &&
	test_cmp expect actual
'

# ── -C with nested path ───────────────────────────────────────────────────

test_expect_success '-C works from deep subdirectory context' '
	git -C normal-repo/a/b rev-parse --is-inside-work-tree >actual &&
	grep "true" actual
'

test_expect_success '-C flag works for status' '
	git -C normal-repo status >actual 2>&1 &&
	grep "On branch" actual
'

# ── rev-parse --show-cdup ─────────────────────────────────────────────────

test_expect_success 'rev-parse HEAD returns valid SHA' '
	cd normal-repo &&
	sha=$(git rev-parse HEAD) &&
	test ${#sha} -eq 40 &&
	echo "$sha" | grep -qE "^[0-9a-f]{40}$"
'

test_expect_success 'rev-parse HEAD~0 equals HEAD' '
	cd normal-repo &&
	head=$(git rev-parse HEAD) &&
	head0=$(git rev-parse HEAD~0) &&
	test "$head" = "$head0"
'

# ── init with directory argument ───────────────────────────────────────────

test_expect_success 'init creates directory if needed' '
	git init auto-created-dir/sub &&
	test -d auto-created-dir/sub/.git
'

test_expect_success 'init --bare creates bare repo at path' '
	git init --bare new-bare.git &&
	test -f new-bare.git/HEAD &&
	cd new-bare.git &&
	result=$(git rev-parse --is-bare-repository) &&
	test "$result" = "true"
'

test_done
