#!/bin/sh

test_description='check that certain rev-parse options work outside repo'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Use a temp directory outside the grit worktree for true non-repo tests

test_expect_success 'git rev-parse --git-dir fails outside repo' '
	tmpdir=$(mktemp -d) &&
	(
		cd "$tmpdir" &&
		GIT_CEILING_DIRECTORIES="$tmpdir" &&
		export GIT_CEILING_DIRECTORIES &&
		test_must_fail git rev-parse --git-dir
	) &&
	rm -rf "$tmpdir"
'

test_expect_success 'rev-parse --sq-quote' '
	git rev-parse --sq-quote "hello world" >actual 2>&1
'

test_expect_success 'rev-parse --local-env-vars' '
	git rev-parse --local-env-vars >actual &&
	grep ^GIT_DIR actual
'

test_expect_success 'rev-parse --resolve-git-dir' '
	git init --separate-git-dir repo dir &&
	git rev-parse --resolve-git-dir dir/.git >actual
'

test_done
