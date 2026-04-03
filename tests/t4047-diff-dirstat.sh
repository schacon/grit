#!/bin/sh

test_description='diff --stat and --numstat tests (inspired by dirstat)'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

test_expect_success 'setup' '
	mkdir -p changed &&
	mkdir -p unchanged &&
	echo "line 1" >changed/text &&
	echo "line 1" >unchanged/text &&
	git add . &&
	git commit -m "initial" &&
	echo "CHANGED" >changed/text
'

test_expect_success 'diff --stat shows changed file' '
	git diff --stat >output &&
	grep "changed/text" output &&
	! grep "unchanged/text" output
'

test_expect_success 'diff --numstat shows changed file' '
	git diff --numstat >output &&
	grep "changed/text" output &&
	! grep "unchanged/text" output
'

test_expect_success 'diff --shortstat shows summary' '
	git diff --shortstat >output &&
	grep "1 file changed" output
'

test_expect_success 'diff --name-only shows changed file' '
	git diff --name-only >output &&
	grep "changed/text" output &&
	! grep "unchanged/text" output
'

test_expect_success 'diff --name-status shows changed file with status' '
	git diff --name-status >output &&
	grep "M" output &&
	grep "changed/text" output
'

test_done
