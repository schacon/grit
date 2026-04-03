#!/bin/sh

test_description='credential-cache tests'

. ./test-lib.sh

# credential-cache requires a running daemon and unix sockets.
# Grit may not implement this feature.

test_expect_success 'setup' '
	git init
'

test_expect_success 'credential-cache daemon (requires unix sockets)' '
	git credential-cache --timeout=60 exit
'

test_done
