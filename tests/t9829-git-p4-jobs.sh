#!/bin/sh
#
# Upstream: t9829-git-p4-jobs.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 retrieve job info'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'add p4 jobs' '
	false
'

test_expect_failure 'add p4 files' '
	false
'

test_expect_failure 'check log message of changelist with no jobs' '
	false
'

test_expect_failure 'add TESTJOB-A to change 1' '
	false
'

test_expect_failure 'check log message of changelist with one job' '
	false
'

test_expect_failure 'add TESTJOB-B to change 1' '
	false
'

test_expect_failure 'check log message of changelist with more jobs' '
	false
'

test_done
