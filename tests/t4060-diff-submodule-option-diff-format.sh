#!/bin/sh
#
# Copyright (c) 2016 Jacob Keller
#

test_description='Support for diff format verbose submodule difference in git diff'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup superproject and submodule' '
	git init super &&
	cd super &&
	echo base >file &&
	git add file &&
	test_tick &&
	git commit -m "base" &&
	git init sub &&
	cd sub &&
	echo first >content &&
	git add content &&
	test_tick &&
	git commit -m "first"
'

test_expect_success 'add submodule to superproject' '
	cd super &&
	git add sub &&
	test_tick &&
	git commit -m "add sub"
'

test_expect_success 'change in submodule detected by diff' '
	cd super &&
	cd sub &&
	echo second >content &&
	git add content &&
	test_tick &&
	git commit -m "second" &&
	cd .. &&
	git diff --name-only >actual &&
	test -s actual
'

test_expect_success 'diff --stat shows submodule change' '
	cd super &&
	git diff --stat >actual &&
	test -s actual
'

test_done
