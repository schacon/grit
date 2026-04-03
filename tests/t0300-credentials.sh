#!/bin/sh

test_description='basic credential helper tests'

. ./test-lib.sh

test_expect_success 'setup' '
	git init
'

test_expect_success 'credential fill with verbatim helper' '
	write_script git-credential-test-helper <<-\EOF &&
	while read line; do
		test -z "$line" && break
	done
	echo "username=testuser"
	echo "password=testpass"
	EOF
	PATH="$TRASH_DIRECTORY:$PATH" && export PATH &&
	echo "protocol=https
host=example.com
" | git -c credential.helper=test-helper credential fill >actual &&
	grep "username=testuser" actual &&
	grep "password=testpass" actual
'

test_expect_success 'credential fill passes through protocol and host' '
	write_script git-credential-echo <<-\EOF &&
	while read line; do
		echo "$line" >&2
		test -z "$line" && break
	done
	echo "username=user"
	echo "password=pass"
	EOF
	PATH="$TRASH_DIRECTORY:$PATH" && export PATH &&
	echo "protocol=https
host=example.com
" | git -c credential.helper=echo credential fill >stdout 2>stderr &&
	grep "protocol=https" stderr &&
	grep "host=example.com" stderr
'

test_expect_success 'credential approve does not error' '
	echo "protocol=https
host=example.com
username=user
password=pass
" | git credential approve
'

test_expect_success 'credential reject does not error' '
	echo "protocol=https
host=example.com
username=user
password=pass
" | git credential reject
'

test_expect_success 'credential fill with no helper returns empty credentials' '
	echo "protocol=https
host=nohelper.example.com
" | git credential fill >actual &&
	grep "protocol=https" actual &&
	grep "host=nohelper.example.com" actual
'

test_expect_success 'credential fill parses input correctly' '
	echo "protocol=https
host=example.com
path=repo.git
" | git credential fill >actual &&
	grep "protocol=https" actual &&
	grep "host=example.com" actual
'

test_done
