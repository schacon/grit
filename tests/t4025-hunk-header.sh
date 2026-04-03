#!/bin/sh

test_description='diff hunk header truncation'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	echo "int func(void) {" >file &&
	echo "  return 0;" >>file &&
	echo "}" >>file &&
	git add file &&
	git commit -m initial
'

test_expect_success 'diff shows hunk header' '
	echo "  return 1;" >>file &&
	git diff >actual &&
	grep "@@" actual
'

test_done
