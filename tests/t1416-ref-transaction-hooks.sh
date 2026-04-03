#!/bin/sh

test_description='reference transaction hooks'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "t@t" &&
	test_commit PRE &&
	git rev-parse PRE >PRE_OID_file &&
	test_commit POST &&
	git rev-parse POST >POST_OID_file
'

test_expect_success 'hook allows updating ref if successful' '
	PRE_OID=$(cat PRE_OID_file) && POST_OID=$(cat POST_OID_file) &&
	git reset --hard PRE &&
	test_hook reference-transaction <<-\EOF &&
		echo "$*" >>actual
	EOF
	cat >expect <<-EOF &&
		preparing
		prepared
		committed
	EOF
	git update-ref HEAD $POST_OID &&
	test_cmp expect actual
'

test_expect_success 'hook aborts updating ref in preparing state' '
	PRE_OID=$(cat PRE_OID_file) && POST_OID=$(cat POST_OID_file) &&
	git reset --hard PRE &&
	test_hook reference-transaction <<-\EOF &&
		if test "$1" = preparing
		then
			exit 1
		fi
	EOF
	test_must_fail git update-ref HEAD $POST_OID
'

test_done
