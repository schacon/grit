#!/bin/sh
#
# Copyright (c) 2009 Jens Lehmann, based on t7401 by Ping Yin
#

test_description='Support for verbose submodule differences in git diff'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup - create superproject' '
	git init super &&
	cd super &&
	echo file > file &&
	git add file &&
	test_tick &&
	git commit -m "initial"
'

test_expect_success 'setup - create submodule' '
	cd super &&
	git init sub &&
	cd sub &&
	echo sub-content > subfile &&
	git add subfile &&
	test_tick &&
	git commit -m "submodule initial"
'

test_expect_success 'add submodule and commit' '
	cd super &&
	git add sub &&
	test_tick &&
	git commit -m "add submodule"
'

test_expect_success 'diff after submodule change' '
	cd super &&
	cd sub &&
	echo updated > subfile &&
	git add subfile &&
	test_tick &&
	git commit -m "update submodule" &&
	cd .. &&
	git diff --name-only >actual &&
	test -s actual
'

test_expect_success 'diff --stat works' '
	cd super &&
	git diff --stat >actual &&
	test -s actual
'

test_done
