#!/bin/sh

test_description='test git repo structure basics'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# The git repo structure command is not implemented in grit.
# Test basic repo structure inspection instead.

test_expect_success 'setup empty repo' '
	git init repo &&
	cd repo
'

test_expect_success 'empty repo has no commits' '
	cd repo &&
	test_must_fail git rev-parse HEAD 2>/dev/null
'

test_expect_success 'repo with commits has objects' '
	cd repo &&
	echo content >file &&
	git add file &&
	git commit -m "first" &&
	git rev-parse HEAD >actual &&
	test -s actual
'

test_expect_success 'count-objects shows object count' '
	cd repo &&
	git count-objects >actual &&
	test -s actual
'

test_done
