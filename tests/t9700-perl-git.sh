#!/bin/sh
#
# Upstream: t9700-perl-git.sh
# Requires Perl Git bindings — ported as test_expect_failure stubs.
#

test_description='perl interface (Git.pm)'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

skip_all='Perl Git bindings not available in grit'
test_done
