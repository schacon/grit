#!/bin/sh
<<<<<<< HEAD
# Ported from git/t/t7426-submodule-get-default-remote.sh
# git submodule--helper get-default-remote

test_description='git submodule--helper get-default-remote'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'portable — not yet ported' '
=======
#
# Upstream: t7426-submodule-get-default-remote.sh
# Requires submodule--helper which is not yet implemented in grit.
# Stubbed as test_expect_failure.
#

test_description='git submodule--helper get-default-remote'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- submodule--helper not yet available in grit ---

test_expect_failure 'setup repositories' '
	false
'

test_expect_failure 'get-default-remote returns origin for initialized submodule' '
	false
'

test_expect_failure 'get-default-remote works from subdirectory' '
	false
'

test_expect_failure 'get-default-remote fails with non-existent path' '
	false
'

test_expect_failure 'get-default-remote fails with non-submodule path' '
	false
'

test_expect_failure 'get-default-remote fails without path argument' '
	false
'

test_expect_failure 'get-default-remote fails with too many arguments' '
	false
'

test_expect_failure 'get-default-remote returns non-origin remote name' '
	false
'

test_expect_failure 'get-default-remote handles submodule with multiple remotes' '
	false
'

test_expect_failure 'get-default-remote handles submodule with multiple remotes and none are origin' '
	false
'

test_expect_failure 'get-default-remote works with nested submodule' '
	false
'

test_expect_failure 'get-default-remote works with submodule that has no remotes' '
>>>>>>> test/batch-EN
	false
'

test_done
