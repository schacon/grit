#!/bin/sh
# Ported from git/t/t9602-cvsimport-branches-tags.sh
# git cvsimport handling of branches and tags

test_description='git cvsimport handling of branches and tags'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup: init repo' 'git init -q'

test_expect_failure 'cvsimport (requires CVS) — not yet ported' '
	false
'

test_done
