#!/bin/sh
#
# Upstream: t9700-perl-git.sh
# Requires Perl Git bindings — ported as test_expect_failure stubs.
#

test_description='perl interface (Git.pm)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perl Git bindings not available in grit ---

test_expect_failure 'set up test repository' '
	false
'

test_expect_failure 'set up bare repository' '
	false
'

test_expect_failure 'use t9700/test.pl to test Git.pm' '
	false
'

test_done
