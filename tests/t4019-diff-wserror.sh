#!/bin/sh

test_description='diff whitespace error detection'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

test_expect_success setup '
	>F &&
	git add F &&
	echo "         Eight SP indent" >>F &&
	echo " 	HT and SP indent" >>F &&
	printf "With trailing SP \n" >>F &&
	echo "No problem" >>F &&
	echo >>F
'

test_expect_success 'diff --color produces output' '
	git diff --color >output &&
	test -s output
'

test_expect_success 'diff output contains added lines' '
	git diff >output &&
	grep "Eight SP indent" output &&
	grep "HT and SP indent" output &&
	grep "No problem" output
'

test_done
