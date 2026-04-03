#!/bin/sh

test_description='check receive input limits'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	dd if=/dev/urandom bs=1024 count=1 2>/dev/null >one-k &&
	git add one-k &&
	git commit -m one-k
'

test_expect_success 'send-pack to bare dest succeeds' '
	rm -fr dest &&
	git init --bare dest &&
	git send-pack ./dest master
'

test_expect_success 'send-pack to bare dest with more content' '
	dd if=/dev/urandom bs=1024 count=2 2>/dev/null >two-k &&
	git add two-k &&
	git commit -m two-k &&
	rm -fr dest &&
	git init --bare dest &&
	git send-pack ./dest master
'

test_done
