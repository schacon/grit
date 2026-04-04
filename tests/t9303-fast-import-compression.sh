#!/bin/sh
#
# Upstream: t9303-fast-import-compression.sh
# Requires fast-import — ported as test_expect_failure stubs.
#

test_description='compression setting of fast-import utility'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- fast-import not available in grit ---

test_expect_failure 'fast-import (packed) with $config' '
	false
'

test_expect_failure 'fast-import (loose) with $config' '
	false
'

test_done
