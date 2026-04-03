#!/bin/sh
#
# Copyright (c) 2016 Jacob Keller
#

test_description='Test for submodule diff on non-checked out submodule'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup - create superproject and submodule' '
	git init super &&
	cd super &&
	echo frotz >nitfol &&
	git add nitfol &&
	test_tick &&
	git commit -m "superproject initial" &&
	git init sub &&
	cd sub &&
	echo hello >world &&
	git add world &&
	test_tick &&
	git commit -m "submodule initial"
'

test_expect_failure 'add submodule' '
	cd super &&
	git add sub &&
	test_tick &&
	git commit -m "add submodule"
'

test_expect_failure 'diff-tree after adding submodule' '
	cd super &&
	git diff-tree -r --name-only HEAD~1 HEAD >actual &&
	test -s actual
'

test_done
