#!/bin/sh

test_description='git log with various edge cases'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init -q &&
	git config user.name "A U Thor" &&
	git config user.email "author@example.com" &&

	echo content >file &&
	git add file &&
	test_tick &&
	git commit -q -m "initial"
'

test_expect_success 'log -n 1 limits to one commit' '
	git log -n 1 --format="%s" >actual &&
	test_line_count = 1 actual
'

test_expect_success 'log --skip=999 produces no output' '
	git log --skip=999 --format="%s" >actual &&
	test_must_be_empty actual
'

test_expect_success 'log --oneline format is consistent' '
	git log --oneline >actual &&
	test_line_count = 1 actual &&
	hash=$(git rev-parse --short HEAD) &&
	grep "^$hash " actual
'

test_expect_success 'log format %H matches rev-parse' '
	expected=$(git rev-parse HEAD) &&
	actual=$(git log -n 1 --format="%H") &&
	test "$expected" = "$actual"
'

test_expect_success 'log format %h is prefix of %H' '
	full=$(git log -n 1 --format="%H") &&
	short=$(git log -n 1 --format="%h") &&
	case "$full" in
	$short*) true ;;
	*) false ;;
	esac
'

test_expect_success 'log with nonexistent revision fails' '
	test_must_fail git log --format="%s" nonexistent 2>err &&
	test -s err
'

test_done
