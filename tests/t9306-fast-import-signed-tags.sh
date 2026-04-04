#!/bin/sh
#
# Upstream: t9306-fast-import-signed-tags.sh
# Requires fast-import — ported as test_expect_failure stubs.
#

test_description='git fast-import --signed-tags=<mode>'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# --- fast-import not available in grit ---

test_expect_failure 'set up unsigned initial commit and import repo' '
	false
'

test_expect_failure 'import no signed tag with --signed-tags=abort' '
	false
'

test_expect_failure 'set up OpenPGP signed tag' '
	false
'

test_expect_failure 'import OpenPGP signed tag with --signed-tags=abort' '
	false
'

test_expect_failure 'import OpenPGP signed tag with --signed-tags=verbatim' '
	false
'

test_expect_failure 'setup X.509 signed tag' '
	false
'

test_expect_failure 'import X.509 signed tag with --signed-tags=warn-strip' '
	false
'

test_expect_failure 'setup SSH signed tag' '
	false
'

test_expect_failure 'import SSH signed tag with --signed-tags=warn-verbatim' '
	false
'

test_expect_failure 'import SSH signed tag with --signed-tags=strip' '
	false
'

test_done
