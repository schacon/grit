#!/bin/sh
#
# Upstream: t9901-git-web--browse.sh
# Requires web--browse — ported as test_expect_success stubs.
#

test_description='git web--browse basic tests

This test checks that git web--browse can handle various valid URLs.'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- web--browse not available in grit ---

test_expect_failure 'URL with an ampersand in it' '
	false
'

test_expect_failure 'URL with a semi-colon in it' '
	false
'

test_expect_failure 'URL with a hash in it' '
	false
'

test_expect_failure 'browser paths are properly quoted' '
	false
'

test_expect_failure 'browser command allows arbitrary shell code' '
	false
'

test_done
