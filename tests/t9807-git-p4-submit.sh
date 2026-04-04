#!/bin/sh
#
# Upstream: t9807-git-p4-submit.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 submit'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- Perforce not available in grit ---

test_expect_failure 'start p4d' '
	false
'

test_expect_failure 'init depot' '
	false
'

test_expect_failure 'is_cli_file_writeable function' '
	false
'

test_expect_failure 'submit with no client dir' '
	false
'

test_expect_failure 'submit --origin' '
	false
'

test_expect_failure 'submit --dry-run' '
	false
'

test_expect_failure 'submit --dry-run --export-labels' '
	false
'

test_expect_failure 'submit with allowSubmit' '
	false
'

test_expect_failure 'submit with master branch name from argv' '
	false
'

test_expect_failure 'allow submit from branch with same revision but different name' '
	false
'

test_expect_failure 'submit --commit one' '
	false
'

test_expect_failure 'submit --commit range' '
	false
'

test_expect_failure 'submit modify' '
	false
'

test_expect_failure 'submit add' '
	false
'

test_expect_failure 'submit delete' '
	false
'

test_expect_failure 'submit copy' '
	false
'

test_expect_failure 'submit rename' '
	false
'

test_expect_failure 'simple one-line description' '
	false
'

test_expect_failure 'description with odd formatting' '
	false
'

test_expect_failure 'description with Jobs section at end' '
	false
'

test_expect_failure 'description with Jobs and values on separate lines' '
	false
'

test_expect_failure 'description with Jobs section and bogus following text' '
	false
'

test_expect_failure 'submit --prepare-p4-only' '
	false
'

test_expect_failure 'submit --shelve' '
	false
'

test_expect_failure 'submit --update-shelve' '
	false
'

test_expect_failure 'update a shelve involving moved and copied files' '
	false
'

test_done
