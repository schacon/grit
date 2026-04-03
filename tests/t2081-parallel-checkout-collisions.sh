#!/bin/sh

test_description='path collisions during checkout'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Test basic checkout behavior with conflicting paths

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	echo content >file &&
	git add file &&
	git commit -m "initial"
'

test_expect_success 'checkout -- restores file from index' '
	cd repo &&
	echo local-change >file &&
	git checkout -- file &&
	test "$(cat file)" = "content"
'

test_expect_success 'checkout -f forces overwrite' '
	cd repo &&
	echo local-change >file &&
	git checkout -- file &&
	test "$(cat file)" = "content"
'

test_done
