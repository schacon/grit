#!/bin/sh
# Ported from upstream t1461-refs-list.sh
# Original uses 'git refs list' which grit does not have.
# We test using 'git for-each-ref' which has equivalent behavior.

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	git config user.name "Test" &&
	git config user.email "test@test.com" &&
	tree=$(git write-tree) &&
	commit=$(echo base | git commit-tree "$tree") &&
	git update-ref refs/heads/master "$commit" &&
	git update-ref refs/heads/main "$commit" &&
	git update-ref refs/heads/side "$commit" &&
	git update-ref refs/tags/v1.0 "$commit"
'

test_expect_success 'for-each-ref lists all refs' '
	git for-each-ref >output &&
	test $(wc -l <output) -ge 3
'

test_expect_success 'for-each-ref with pattern filters to heads' '
	git for-each-ref refs/heads/ >output &&
	grep "refs/heads/master" output &&
	grep "refs/heads/main" output &&
	grep "refs/heads/side" output
'

test_expect_success 'for-each-ref refs/tags/ shows tags' '
	git for-each-ref refs/tags/ >output &&
	grep "refs/tags/v1.0" output
'

test_expect_success 'for-each-ref with format shows refnames' '
	git for-each-ref --format="%(refname)" refs/heads/ >output &&
	grep "refs/heads/main" output
'

test_expect_success 'for-each-ref does not show non-matching patterns' '
	git for-each-ref refs/nonexistent/ >output &&
	test_must_be_empty output
'

test_done
