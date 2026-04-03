#!/bin/sh

test_description='Test git update-server-info'

. ./test-lib.sh

test_expect_success 'setup' '
	git init &&
	test_commit file
'

test_expect_success 'create info/refs' '
	git update-server-info &&
	test_path_is_file .git/info/refs
'

test_expect_success 'info/refs can be forced to update' '
	git update-server-info -f &&
	test_path_is_file .git/info/refs
'

test_expect_success 'info/refs updates when changes are made' '
	cp .git/info/refs before &&
	git update-ref refs/heads/foo HEAD &&
	git update-server-info &&
	! test_cmp before .git/info/refs
'

test_done
