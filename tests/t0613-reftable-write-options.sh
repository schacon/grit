#!/bin/sh

test_description='reftable write options'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# grit does not yet implement the reftable backend with write options.
# All reftable write option tests are expected failures.

test_expect_failure 'default write options' '
	false
'

test_expect_failure 'disabled reflog writes no log blocks' '
	false
'

test_expect_failure 'block-size option' '
	false
'

test_done
