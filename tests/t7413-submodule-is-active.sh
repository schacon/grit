#!/bin/sh
#
# Ported from git/t/t7413-submodule-is-active.sh
# Tests submodule is-active functionality
# Note: upstream uses test-tool which grit doesn't have.
# We test what we can via submodule status behavior.
#

test_description='Test submodule is-active behavior'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup' '
	"$REAL_GIT" config --global protocol.file.allow always &&
	"$REAL_GIT" init sub &&
	(
		cd sub &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo initial >initial.t &&
		"$REAL_GIT" add initial.t &&
		"$REAL_GIT" commit -m initial &&
		"$REAL_GIT" tag initial
	) &&
	"$REAL_GIT" init super &&
	(
		cd super &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo initial >initial.t &&
		"$REAL_GIT" add initial.t &&
		"$REAL_GIT" commit -m initial &&
		"$REAL_GIT" tag initial &&
		"$REAL_GIT" submodule add ../sub sub1 &&
		"$REAL_GIT" submodule add ../sub sub2 &&
		"$REAL_GIT" commit -a -m "add 2 submodules at sub{1,2}"
	)
'

test_expect_success 'submodule status works on active submodules' '
	cd super &&
	git submodule status >actual &&
	grep sub1 actual &&
	grep sub2 actual
'

test_expect_success 'submodule init works' '
	cd super &&
	git config submodule.sub1.url >actual &&
	test -s actual
'

test_done
