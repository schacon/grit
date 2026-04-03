#!/bin/sh

test_description='diff hunk header truncation'

. ./test-lib.sh

test_expect_success 'setup: init repo' '
	git init -q &&
	git config user.name "Test" &&
	git config user.email "test@example.com"
'

N="日本語"
NS="$N$N$N$N$N$N$N$N$N$N$N$N$N"

test_expect_success setup '
	(
		echo "A $NS" &&
		printf "  %s\n" B C D E F G H I J K &&
		echo "L  $NS" &&
		printf "  %s\n" M N O P Q R S T U V
	) >file &&
	git add file &&
	sed -e "/^  [EP]/s/$/ modified/" <file >file+ &&
	mv file+ file
'

test_expect_success 'hunk header includes funcname context' '
	git diff >output &&
	grep "^@@.*@@" output >headers &&
	test_line_count = 2 headers
'

test_done
