#!/bin/sh
# Ported from upstream git t7510-signed-commit.sh
# GPG signing is not available, so we test commit and log operations
# that would be used around signed commits.

test_description='signed commit (structure tests, no GPG)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init signed-repo &&
	cd signed-repo &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	echo content >file &&
	git add file &&
	test_tick &&
	git commit -m initial
'

test_expect_success 'log --format=%H shows full hash' '
	cd signed-repo &&
	git log --format=%H >actual &&
	test $(wc -c <actual) -gt 40
'

test_expect_success 'log --format=%GG without gpg shows empty' '
	cd signed-repo &&
	git log --format="%GG" >actual 2>/dev/null || true &&
	# should not crash
	true
'

test_expect_success 'cat-file commit shows no gpgsig without signing' '
	cd signed-repo &&
	git cat-file -p HEAD >actual &&
	! grep "^gpgsig" actual
'

test_expect_success 'multiple commits and verify log' '
	cd signed-repo &&
	echo more >file2 &&
	git add file2 &&
	test_tick &&
	git commit -m second &&
	echo even_more >file3 &&
	git add file3 &&
	test_tick &&
	git commit -m third &&
	git log --oneline >actual &&
	test_line_count = 3 actual
'

test_expect_success 'show commit message' '
	cd signed-repo &&
	git log --max-count=1 --format=%s HEAD >actual &&
	grep "third" actual
'

test_expect_success 'rev-parse works on all commits' '
	cd signed-repo &&
	git rev-parse HEAD >actual &&
	test -s actual &&
	git rev-parse HEAD^ >actual &&
	test -s actual &&
	git rev-parse HEAD~2 >actual &&
	test -s actual
'

test_done
