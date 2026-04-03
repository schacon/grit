#!/bin/sh
#
# Ported from git/t/t0033-safe-directory.sh (subset)
# Tests safe.directory config without ownership mocking

test_description='verify safe.directory checks'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q
'

test_expect_success 'safe.directory on the command line' '
	git -c safe.directory="$(pwd)" status
'

test_expect_success 'safe.directory with glob matching' '
	p=$(pwd) &&
	git config --global safe.directory "${p%/*}/*" &&
	git status
'

test_expect_success 'safe.directory with star matches everything' '
	git config --global --unset-all safe.directory &&
	git config --global safe.directory "*" &&
	git status
'

test_done
