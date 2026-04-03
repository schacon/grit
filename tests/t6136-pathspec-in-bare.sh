#!/bin/sh

test_description='diagnosing out-of-scope pathspec in bare repos'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup a bare and non-bare repository' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	test_commit file1 &&
	git clone --bare . ../bare
'

test_expect_success 'log works in bare repo' '
	cd bare &&
	git log --oneline >output &&
	test_line_count = 1 output
'

test_expect_success 'ls-files works in bare repo' '
	cd bare &&
	git ls-files >output 2>err || true
'

test_done
