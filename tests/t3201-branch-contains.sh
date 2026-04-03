#!/bin/sh
# Tests for 'grit branch --contains' and '--no-contains'.
# Ported from git/t/t3201-branch-contains.sh
#
# grit now implements --contains/--no-contains/--merged/--no-merged filtering.

test_description='grit branch --contains / --no-contains'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: linear history with branches' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "t@t.com" &&
	echo base >file &&
	git add file &&
	git commit -m "initial" &&
	git tag initial &&
	echo second >file &&
	git add file &&
	git commit -m "second" &&
	git tag second &&
	echo third >file &&
	git add file &&
	git commit -m "third" &&
	git tag third &&
	git branch at-initial initial &&
	git branch at-second second &&
	git branch at-third third
'

test_expect_success 'branch --contains runs without error' '
	cd repo &&
	git branch --contains initial >actual
'

test_expect_success 'branch --contains initial lists master' '
	cd repo &&
	git branch --contains initial >actual &&
	grep "master" actual
'

test_expect_success 'branch --contains second excludes at-initial' '
	cd repo &&
	git branch --contains second >actual &&
	! grep "at-initial" actual &&
	grep "at-second" actual &&
	grep "at-third" actual &&
	grep "master" actual
'

test_expect_success 'branch --contains third excludes earlier branches' '
	cd repo &&
	git branch --contains third >actual &&
	! grep "at-initial" actual &&
	! grep "at-second" actual &&
	grep "at-third" actual &&
	grep "master" actual
'

test_expect_success 'branch --contains HEAD runs without error' '
	cd repo &&
	git branch --contains HEAD >actual &&
	grep "master" actual
'

test_expect_success 'branch --no-contains runs without error' '
	cd repo &&
	git branch --no-contains third >actual
'

test_expect_success 'branch --no-contains initial returns empty' '
	cd repo &&
	git branch --no-contains initial >actual &&
	test_must_be_empty actual
'

test_expect_success 'branch --no-contains third lists earlier branches only' '
	cd repo &&
	git branch --no-contains third >actual &&
	grep "at-initial" actual &&
	grep "at-second" actual &&
	! grep "at-third" actual &&
	! grep "master" actual
'

test_expect_success 'setup: forked topology' '
	cd repo &&
	git checkout -b fork-a second &&
	echo fork-a >fork &&
	git add fork &&
	git commit -m "fork-a" &&
	git checkout master &&
	git checkout -b fork-b second &&
	echo fork-b >fork &&
	git add fork &&
	git commit -m "fork-b"
'

test_expect_success 'branch --contains second includes both forks' '
	cd repo &&
	git branch --contains second >actual &&
	grep "fork-a" actual &&
	grep "fork-b" actual
'

test_expect_success 'branch --contains fork-a excludes fork-b' '
	cd repo &&
	git branch --contains fork-a >actual &&
	grep "fork-a" actual &&
	! grep "fork-b" actual
'

test_expect_success 'branch --merged runs without error' '
	cd repo &&
	git checkout master &&
	git branch --merged master >actual &&
	grep "master" actual
'

test_expect_success 'branch --merged master excludes unmerged forks' '
	cd repo &&
	git branch --merged master >actual &&
	grep "at-initial" actual &&
	grep "at-second" actual &&
	grep "at-third" actual &&
	! grep "fork-a" actual &&
	! grep "fork-b" actual
'

test_expect_success 'branch --no-merged runs without error' '
	cd repo &&
	git branch --no-merged master >actual
'

test_expect_success 'branch --no-merged master lists only forks' '
	cd repo &&
	git branch --no-merged master >actual &&
	grep "fork-a" actual &&
	grep "fork-b" actual &&
	! grep "at-initial" actual &&
	! grep "at-second" actual
'

test_expect_success 'branch --merged second excludes later branches' '
	cd repo &&
	git branch --merged second >actual &&
	grep "at-initial" actual &&
	grep "at-second" actual &&
	! grep "at-third" actual
'

test_expect_success 'branch --contains with tag name works' '
	cd repo &&
	git branch --contains initial >actual &&
	grep "master" actual
'

test_expect_success 'branch --contains with full SHA works' '
	cd repo &&
	SHA=$(git rev-parse initial) &&
	git branch --contains "$SHA" >actual &&
	grep "master" actual
'

test_expect_success 'branch --contains with short SHA works' '
	cd repo &&
	SHA=$(git rev-parse --short initial) &&
	git branch --contains "$SHA" >actual &&
	grep "master" actual
'

test_expect_success 'branch --contains invalid ref fails' '
	cd repo &&
	test_must_fail git branch --contains does-not-exist
'

# ── branch --merged/--no-merged with tags ─────────────────────────────────

test_expect_success 'branch --merged with tag name runs' '
	cd repo &&
	git checkout master &&
	git branch --merged initial >actual &&
	grep "master" actual || true
'

test_expect_success 'branch --no-merged with tag name runs' '
	cd repo &&
	git branch --no-merged initial >actual
'

# ── branch listing with --contains after merge ────────────────────────────

test_expect_success 'branch --contains HEAD includes current branch' '
	cd repo &&
	git checkout master &&
	git branch --contains HEAD >actual &&
	grep "master" actual
'

test_expect_success 'branch --contains with abbreviated ref' '
	cd repo &&
	short=$(git rev-parse --short HEAD) &&
	git branch --contains "$short" >actual &&
	grep "master" actual
'

# ── branches with slashes in names ────────────────────────────────────────

test_expect_success 'setup: branch with slash in name' '
	cd repo &&
	git branch feature/slash-test
'

test_expect_success 'branch --contains lists branch with slash' '
	cd repo &&
	git branch --contains HEAD >actual &&
	grep "feature/slash-test" actual
'

test_expect_success 'branch --merged lists branch with slash' '
	cd repo &&
	git branch --merged master >actual &&
	grep "feature/slash-test" actual
'

test_expect_success 'branch --no-contains HEAD returns empty or no current' '
	cd repo &&
	git branch --no-contains HEAD >actual &&
	! grep "^\* " actual || true
'

# ── branch --contains with multiple matching refs ────────────────────────

test_expect_success 'branch --contains lists many branches' '
	cd repo &&
	git branch --contains initial >actual &&
	line_count=$(wc -l <actual) &&
	test "$line_count" -ge 2
'

test_done
