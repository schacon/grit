#!/bin/sh
#
# Upstream: t9302-fast-import-unpack-limit.sh
# Requires fast-import — ported as test_expect_failure stubs.
#

test_description='test git fast-import unpack limit'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- fast-import not available in grit ---

test_expect_failure 'create loose objects on import' '
	false
'

test_expect_failure 'bigger packs are preserved' '
	false
'

test_expect_failure 'lookups after checkpoint works' '
	false
'

test_done
