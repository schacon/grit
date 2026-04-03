#!/bin/sh

test_description='rewrite diff'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test User" &&
	git config user.email "test@example.com"
'

test_expect_success 'show deletion diff' '
	echo "to be deleted" >test2 &&
	git add test2 &&
	git commit -m initial &&
	rm test2 &&
	git diff >actual &&
	grep "deleted file mode" actual &&
	grep "to be deleted" actual
'

test_done
