#!/bin/sh
#
# Ported from git/t/t0031-lockfile-pid.sh (subset)

test_description='lock file PID info tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'PID info file cleaned up on successful operation when enabled' '
	git init repo4 &&
	(
		cd repo4 &&
		echo content >file &&
		git -c core.lockfilePid=true add file &&
		# After successful add, no lock or PID files should exist
		test_path_is_missing .git/index.lock &&
		test_path_is_missing .git/index~pid.lock
	)
'

test_expect_success 'no PID file created by default' '
	git init repo5 &&
	(
		cd repo5 &&
		echo content >file &&
		git add file &&
		# PID file should not be created when feature is disabled
		test_path_is_missing .git/index~pid.lock
	)
'

test_done
