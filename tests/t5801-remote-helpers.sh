#!/bin/sh
#
# Upstream: t5801-remote-helpers.sh
# Tests for remote helper import and export commands.
# Requires 'testgit' remote helper protocol and git-remote-testgit/
# git-remote-nourl test helpers, plus full clone/fetch/push support.
#

test_description='Test remote-helper import and export commands'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='remote helpers (testgit) not available in grit'
test_done
