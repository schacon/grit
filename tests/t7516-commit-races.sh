#!/bin/sh
# Ported from upstream git t7516-commit-races.sh

test_description='git commit races'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init race-repo &&
	cd race-repo &&
	git config user.name "Test" &&
	git config user.email "test@test.com"
'

test_expect_success 'commit --allow-empty works' '
	cd race-repo &&
	test_tick &&
	git commit --allow-empty -m "empty1" &&
	git log --oneline >actual &&
	test_line_count = 1 actual
'

test_expect_success 'multiple empty commits' '
	cd race-repo &&
	test_tick &&
	git commit --allow-empty -m "empty2" &&
	test_tick &&
	git commit --allow-empty -m "empty3" &&
	git log --oneline >actual &&
	test_line_count = 3 actual
'

test_expect_success 'rev-parse HEAD works after commits' '
	cd race-repo &&
	git rev-parse HEAD >actual &&
	test -s actual
'

test_expect_success 'show last commit subject' '
	cd race-repo &&
	git log --max-count=1 --format=%s >subject &&
	grep "empty3" subject
'

test_done
