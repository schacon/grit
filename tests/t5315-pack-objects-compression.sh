#!/bin/sh

test_description='pack-object compression configuration'

. ./test-lib.sh

test_file_size () {
	wc -c <"$1" | tr -d ' '
}

test_expect_success setup '
	git init &&
	printf "%2000000s" X |
	git hash-object -w --stdin >object-name &&
	# make sure it resulted in a loose object
	ob=$(sed -e "s/\(..\).*/\1/" object-name) &&
	ject=$(sed -e "s/..\(.*\)/\1/" object-name) &&
	test -f .git/objects/$ob/$ject
'

test_expect_success 'pack-objects produces compressed output' '
	git pack-objects pack <object-name &&
	sz=$(test_file_size pack-*.pack) &&
	test "$sz" -le 100000
'

test_done
