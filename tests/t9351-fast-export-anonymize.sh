#!/bin/sh
#
# Upstream: t9351-fast-export-anonymize.sh
# Requires fast-export — ported as test_expect_failure stubs.
#

test_description='basic tests for fast-export --anonymize'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- fast-export not available in grit ---

test_expect_failure 'setup simple repo' '
	false
'

test_expect_failure 'export anonymized stream' '
	false
'

test_expect_failure 'stream omits path names' '
	false
'

test_expect_failure 'stream contains user-specified names' '
	false
'

test_expect_failure 'stream omits gitlink oids' '
	false
'

test_expect_failure 'stream retains other as refname' '
	false
'

test_expect_failure 'stream omits other refnames' '
	false
'

test_expect_failure 'stream omits identities' '
	false
'

test_expect_failure 'stream omits tag message' '
	false
'

test_expect_failure 'import stream to new repository' '
	false
'

test_expect_failure 'result has two branches' '
	false
'

test_expect_failure 'repo has original shape and timestamps' '
	false
'

test_expect_failure 'root tree has original shape' '
	false
'

test_expect_failure 'paths in subdir ended up in one tree' '
	false
'

test_expect_failure 'identical gitlinks got identical oid' '
	false
'

test_expect_failure 'all tags point to branch tip' '
	false
'

test_expect_failure 'idents are shared' '
	false
'

test_done
