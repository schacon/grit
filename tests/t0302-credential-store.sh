#!/bin/sh

test_description='credential-store tests'

. ./test-lib.sh

test_expect_success 'setup' '
	git init
'

test_expect_success 'credential-store: store and retrieve credentials' '
	test_when_finished "rm -f cred-file" &&
	echo "protocol=https
host=example.com
username=testuser
password=testpass
" | git credential-store --file cred-file store &&
	test_path_is_file cred-file &&
	echo "protocol=https
host=example.com
" | git credential-store --file cred-file get >actual &&
	grep "username=testuser" actual &&
	grep "password=testpass" actual
'

test_expect_success 'credential-store: erase credentials' '
	test_when_finished "rm -f cred-file" &&
	echo "protocol=https
host=example.com
username=testuser
password=testpass
" | git credential-store --file cred-file store &&
	echo "protocol=https
host=example.com
username=testuser
password=testpass
" | git credential-store --file cred-file erase &&
	echo "protocol=https
host=example.com
" | git credential-store --file cred-file get >actual &&
	! grep "username=testuser" actual
'

test_expect_success 'credential-store: store multiple credentials' '
	test_when_finished "rm -f cred-file" &&
	echo "protocol=https
host=one.example.com
username=user1
password=pass1
" | git credential-store --file cred-file store &&
	echo "protocol=https
host=two.example.com
username=user2
password=pass2
" | git credential-store --file cred-file store &&
	echo "protocol=https
host=one.example.com
" | git credential-store --file cred-file get >actual &&
	grep "username=user1" actual &&
	grep "password=pass1" actual
'

test_expect_success 'credential-store: credentials file format' '
	test_when_finished "rm -f cred-file" &&
	echo "protocol=https
host=example.com
username=testuser
password=testpass
" | git credential-store --file cred-file store &&
	grep "https://testuser:testpass@example.com" cred-file
'

test_done
