#!/bin/sh

test_description='Test Git when git repository is located at root

This test requires write access in root. Do not bother if you do not
have a throwaway chroot or VM.
'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# This test needs root access to create repos at /.
# All tests are skipped in normal environments.

test_expect_success 'rev-parse --git-dir works' '
	git init testrepo &&
	(
		cd testrepo &&
		echo .git >expect &&
		git rev-parse --git-dir >actual &&
		test_cmp expect actual
	)
'

test_expect_success 'rev-parse --show-toplevel works' '
	(
		cd testrepo &&
		pwd >expect &&
		git rev-parse --show-toplevel >actual &&
		test_cmp expect actual
	)
'

test_expect_success 'rev-parse --show-prefix in subdir' '
	mkdir -p testrepo/sub/dir &&
	(
		cd testrepo/sub/dir &&
		echo "sub/dir/" >expect &&
		git rev-parse --show-prefix >actual &&
		test_cmp expect actual
	)
'

test_done
