#!/bin/sh
#
# Upstream: t9835-git-p4-metadata-encoding-python2.sh
# Requires Perforce — ported as test_expect_failure stubs.
#

test_description='git p4 metadata encoding

This test checks that the import process handles inconsistent text
encoding in p4 metadata (author names, commit messages, etc) without
failing, and produces maximally sane output in git.'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perforce not available in grit'
test_done
