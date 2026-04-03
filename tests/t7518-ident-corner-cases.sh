#!/bin/sh
test_description='corner cases in ident strings'
cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init repo &&
	cd repo &&
	git config user.name "Test" &&
	git config user.email "test@test.com"
'

test_expect_success 'commit does not strip trailing dot' '
	cd repo &&
	author_name="Pat Doe Jr." &&
	env GIT_AUTHOR_NAME="$author_name" \
		git commit --allow-empty -m foo &&
	git log -n 1 --format=%an >actual &&
	echo "$author_name" >expected &&
	test_cmp actual expected
'

test_done
