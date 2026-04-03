#!/bin/sh

test_description='difference in submodules'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup submodule' '
	git init super &&
	cd super &&
	echo frotz >nitfol &&
	git add nitfol &&
	test_tick &&
	git commit -m "superproject" &&
	git init sub &&
	cd sub &&
	echo hello >world &&
	git add world &&
	test_tick &&
	git commit -m "submodule"
'

test_expect_failure 'add submodule to index' '
	cd super &&
	git add sub &&
	test_tick &&
	git commit -m "add submodule"
'

test_expect_success 'diff-files shows submodule change' '
	cd super &&
	cd sub &&
	echo goodbye >world &&
	git add world &&
	test_tick &&
	git commit -m "submodule #2" &&
	cd .. &&
	git diff-files >actual &&
	grep "sub" actual
'

test_expect_failure 'diff --name-only shows submodule' '
	cd super &&
	git diff --name-only >actual &&
	grep "sub" actual
'

test_expect_failure 'diff --name-status shows submodule' '
	cd super &&
	git diff --name-status >actual &&
	grep "M.*sub" actual
'

test_done
