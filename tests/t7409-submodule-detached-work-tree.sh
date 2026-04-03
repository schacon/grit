#!/bin/sh
#
# Ported from git/t/t7409-submodule-detached-work-tree.sh
# Tests submodules on detached working trees
#

test_description='Test submodules on detached working tree'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

REAL_GIT="/usr/bin/git"

test_expect_success 'setup' '
	"$REAL_GIT" config --global protocol.file.allow always &&
	"$REAL_GIT" init --bare remote &&
	test_create_repo bundle1 &&
	(
		cd bundle1 &&
		"$REAL_GIT" config user.email "test@example.com" &&
		"$REAL_GIT" config user.name "Test User" &&
		echo shoot >shoot.t &&
		"$REAL_GIT" add shoot.t &&
		"$REAL_GIT" commit -m "shoot" &&
		"$REAL_GIT" tag shoot &&
		"$REAL_GIT" rev-parse --verify HEAD >../expect
	)
'

test_expect_failure 'submodule on detached working tree' '
	mkdir home &&
	(
		cd home &&
		GIT_WORK_TREE="$(pwd)" &&
		GIT_DIR="$(pwd)/.dotfiles" &&
		export GIT_WORK_TREE GIT_DIR &&
		git clone --bare ../remote .dotfiles &&
		git submodule add ../bundle1 .vim/bundle/sogood &&
		(
			unset GIT_WORK_TREE GIT_DIR &&
			cd .vim/bundle/sogood &&
			git rev-parse --verify HEAD >actual &&
			test_cmp ../../../../expect actual
		)
	)
'

test_done
