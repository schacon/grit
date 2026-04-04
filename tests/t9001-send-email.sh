#!/bin/sh
#
# Upstream: t9001-send-email.sh
# Requires 'git send-email' which is not yet implemented in grit.
# Stubbed as test_expect_failure with representative tests.
#

test_description='git send-email'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- send-email not yet available in grit ---

test_expect_failure 'prepare reference tree' '
	false
'

test_expect_failure 'Setup helper tool' '
	false
'

test_expect_failure 'Extract patches' '
	false
'

test_expect_failure 'Send patches' '
	false
'

test_expect_failure 'Verify commandline' '
	false
'

test_expect_failure 'Send patches with --envelope-sender' '
	false
'

test_expect_failure 'setup expect for cc trailer' '
	false
'

test_expect_failure 'cc trailer with various stripping opts' '
	false
'

test_expect_failure 'setup expect for multiline patch' '
	false
'

test_expect_failure 'multiline subject' '
	false
'

test_expect_failure 'send-email --compose' '
	false
'

test_expect_failure 'send-email --validate hook' '
	false
'

test_expect_failure 'send-email --to-cmd' '
	false
'

test_expect_failure 'send-email --cc-cmd' '
	false
'

test_expect_failure 'send-email --8bit-encoding' '
	false
'

test_done
