#!/bin/sh
#
# Upstream: t5580-unc-paths.sh
# Requires UNC paths (Windows) — ported as test_expect_failure stubs.
#

test_description='various Windows-only path tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- UNC paths (Windows) not available in grit ---

test_expect_failure 'clone without file://' '
	false
'

test_expect_failure 'clone with backslashed path' '
	false
'

test_expect_failure 'remote nick cannot contain backslashes' '
	false
'

test_expect_failure 'unc alternates' '
	false
'

test_done
