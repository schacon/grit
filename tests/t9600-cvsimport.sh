#!/bin/sh
#
# Upstream: t9600-cvsimport.sh
# Requires CVS — ported as test_expect_failure stubs.
#

test_description='git cvsimport basic tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='CVS not available in grit'
test_done
