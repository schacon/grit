#!/bin/sh

test_description='rewrite diff'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

test_expect_success 'prepare a file that ends with an incomplete line' '
	seq 1 99 >seq &&
	printf 100 >>seq &&
	git add seq &&
	git commit -m seq
'

test_expect_success 'rewrite the middle 90% of sequence file and terminate with newline' '
	seq 1 5 >seq &&
	seq 9331 9420 >>seq &&
	seq 96 100 >>seq
'

test_expect_success 'no newline at eof is on its own line in diff output' '
	git diff >res &&
	grep "^\\\\ " res &&
	! grep "^..*\\\\ " res
'

test_expect_success 'show deletion diff' '
	echo "to be deleted" >test2 &&
	git add test2 &&
	git commit -m "add test2" &&
	rm test2 &&
	git diff >actual &&
	grep "^-to be deleted" actual
'

test_done
