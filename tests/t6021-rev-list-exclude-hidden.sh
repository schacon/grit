#!/bin/sh

test_description='git rev-list basic ref exclusion tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test" &&
	test_commit commit1 &&
	COMMIT=$(git rev-parse refs/heads/master) &&
	echo "$COMMIT" >../commit_oid &&
	test_commit tag1 &&
	TAG=$(git rev-parse HEAD) &&
	echo "$TAG" >../tag_oid &&
	git branch other HEAD~1
'

test_expect_success 'rev-list HEAD shows correct count' '
	cd repo &&
	git rev-list HEAD >output &&
	test_line_count = 2 output
'

test_expect_success 'rev-list with branch ref' '
	cd repo &&
	git rev-list master >output &&
	test_line_count = 2 output
'

test_expect_success 'rev-list --all includes all refs' '
	cd repo &&
	git rev-list --all >output &&
	test -s output
'

test_expect_success 'rev-list ^ref..HEAD notation' '
	cd repo &&
	git rev-list HEAD~1..HEAD >output &&
	test_line_count = 1 output
'

test_done
