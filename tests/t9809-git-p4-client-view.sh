#!/bin/sh
#
# Upstream: t9809-git-p4-client-view.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 client view'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'init depot' '
	false
'

test_expect_failure 'view wildcard %%n' '
	false
'

test_expect_failure 'view wildcard *' '
	false
'

test_expect_failure 'wildcard ... in the middle' '
	false
'

test_expect_failure 'wildcard ... in the middle and at the end' '
	false
'

test_expect_failure 'basic map' '
	false
'

test_expect_failure 'client view with no mappings' '
	false
'

test_expect_failure 'single file map' '
	false
'

test_expect_failure 'later mapping takes precedence (entire repo)' '
	false
'

test_expect_failure 'later mapping takes precedence (partial repo)' '
	false
'

test_expect_failure 'depot path matching rejected client path' '
	false
'

test_expect_failure 'exclusion wildcard, client rhs same (odd)' '
	false
'

test_expect_failure 'exclusion wildcard, client rhs different (normal)' '
	false
'

test_expect_failure 'exclusion single file' '
	false
'

test_expect_failure 'overlay wildcard' '
	false
'

test_expect_failure 'overlay single file' '
	false
'

test_expect_failure 'exclusion with later inclusion' '
	false
'

test_expect_failure 'quotes on rhs only' '
	false
'

test_expect_failure 'clone --use-client-spec sets useClientSpec' '
	false
'

test_expect_failure 'subdir clone' '
	false
'

test_expect_failure 'subdir clone, submit modify' '
	false
'

test_expect_failure 'subdir clone, submit add' '
	false
'

test_expect_failure 'subdir clone, submit delete' '
	false
'

test_expect_failure 'subdir clone, submit copy' '
	false
'

test_expect_failure 'subdir clone, submit rename' '
	false
'

test_expect_failure 'wildcard files submit back to p4, client-spec case' '
	false
'

test_expect_failure 'reinit depot' '
	false
'

test_expect_failure 'overlay collision setup' '
	false
'

test_expect_failure 'overlay collision 1 to 2' '
	false
'

test_expect_failure 'overlay collision 2 to 1' '
	false
'

test_expect_failure 'overlay collision delete 2' '
	false
'

test_expect_failure 'overlay collision 1 to 2, but 2 deleted' '
	false
'

test_expect_failure 'overlay collision update 1' '
	false
'

test_expect_failure 'overlay collision 1 to 2, but 2 deleted, then 1 updated' '
	false
'

test_expect_failure 'overlay collision delete filecollides' '
	false
'

test_expect_failure 'overlay sync: add colA in dir1' '
	false
'

test_expect_failure 'overlay sync: initial git checkout' '
	false
'

test_expect_failure 'overlay sync: add colA in dir2' '
	false
'

test_expect_failure 'overlay sync: colA content switch' '
	false
'

test_expect_failure 'overlay sync: add colB in dir1' '
	false
'

test_expect_failure 'overlay sync: colB appears' '
	false
'

test_expect_failure 'overlay sync: add/delete colB in dir2' '
	false
'

test_expect_failure 'overlay sync: colB disappears' '
	false
'

test_expect_failure 'overlay sync: cleanup' '
	false
'

test_expect_failure 'overlay sync swap: add colA in dir1' '
	false
'

test_expect_failure 'overlay sync swap: initial git checkout' '
	false
'

test_expect_failure 'overlay sync swap: add colA in dir2' '
	false
'

test_expect_failure 'overlay sync swap: colA no content switch' '
	false
'

test_expect_failure 'overlay sync swap: add colB in dir1' '
	false
'

test_expect_failure 'overlay sync swap: colB appears' '
	false
'

test_expect_failure 'overlay sync swap: add/delete colB in dir2' '
	false
'

test_expect_failure 'overlay sync swap: colB no change' '
	false
'

test_expect_failure 'overlay sync swap: cleanup' '
	false
'

test_expect_failure 'rename files to introduce spaces' '
	false
'

test_expect_failure 'quotes on lhs only' '
	false
'

test_expect_failure 'quotes on both sides' '
	false
'

test_done
