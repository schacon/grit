#!/bin/sh
# Ported from git/t/t5537-fetch-shallow.sh
# Tests fetch/clone from a shallow clone
# Grit shallow clone support is incomplete (--depth accepted but not enforced)

test_description='fetch/clone from a shallow clone'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	test_commit one &&
	test_commit two &&
	test_commit three &&
	test_commit four
'

# grit --depth does not actually limit history
test_expect_success 'clone with depth' '
	git clone --depth 2 . shallow-clone &&
	(
		cd shallow-clone &&
		git log --oneline >actual &&
		test_line_count = 2 actual
	)
'

# grit --depth does not actually limit history
test_expect_success 'fetch from full to shallow' '
	git init --bare full.git &&
	git push ./full.git main &&
	git clone --depth 1 ./full.git shallow-from-full &&
	(
		cd shallow-from-full &&
		git log --oneline >actual &&
		test_line_count = 1 actual
	)
'

# grit fetch --depth does not limit history
test_expect_success 'fetch --depth from full repo' '
	git clone . depth-test &&
	(
		cd depth-test &&
		git fetch --depth 2 origin main &&
		git log --oneline origin/main >actual &&
		test_line_count = 2 actual
	)
'

# grit does not support --deepen
test_expect_success 'fetch --deepen from full repo' '
	git clone --depth 2 . deepen-test &&
	(
		cd deepen-test &&
		git fetch --deepen 1 origin main &&
		git log --oneline origin/main >actual &&
		test_line_count = 3 actual
	)
'

test_expect_success 'clone full history works' '
	git clone . full-clone &&
	(
		cd full-clone &&
		git log --oneline >actual &&
		test_line_count = 4 actual
	)
'

test_expect_success 'fetch updates refs correctly' '
	test_commit five &&
	(
		cd full-clone &&
		git fetch origin &&
		git log --oneline origin/main >actual &&
		test_line_count = 5 actual
	)
'

test_done
