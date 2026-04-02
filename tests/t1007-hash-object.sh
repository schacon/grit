#!/bin/sh
# Ported from git/t/t1007-hash-object.sh (harness-compatible subset).

test_description='grit hash-object'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

echo_without_newline() {
	printf '%s' "$*"
}

hello_oid='5e1c309dae7f45e0f39b1bf3ac3cd9db12e7d689'
example_oid='ddd3f836d3e3fbb7ae289aa9ae83536f76956399'

setup_repo() {
	grit init --quiet test &&
	cd test &&
	echo_without_newline "Hello World" >hello &&
	echo_without_newline "This is an example" >example
}

teardown_repo() {
	cd .. &&
	rm -rf test
}

test_expect_success 'setup known-content fixtures' '
	echo_without_newline "Hello World" >hello &&
	echo_without_newline "This is an example" >example
'

test_expect_success "multiple '--stdin's are rejected" '
	echo example | test_must_fail grit hash-object --stdin --stdin
'

test_expect_success "cannot use --stdin and --stdin-paths together" '
	echo example | test_must_fail grit hash-object --stdin --stdin-paths &&
	echo example | test_must_fail grit hash-object --stdin-paths --stdin
'

test_expect_success "cannot pass filenames with --stdin-paths" '
	echo example | test_must_fail grit hash-object --stdin-paths hello
'

test_expect_success "cannot use --path with --stdin-paths" '
	echo example | test_must_fail grit hash-object --stdin-paths --path=foo
'

test_expect_success "cannot use --path with --no-filters" '
	test_must_fail grit hash-object --no-filters --path=foo
'

test_expect_success 'hash a file' '
	setup_repo &&
	test "$hello_oid" = "$(grit hash-object hello)"
'

test_expect_success 'hash from stdin' '
	test "$example_oid" = "$(grit hash-object --stdin <example)"
'

test_expect_success 'hash a file and write to database' '
	test "$hello_oid" = "$(grit hash-object -w hello)"
'

test_expect_success 'written blob exists in database' '
	grit cat-file "$hello_oid" >/dev/null
'

test_expect_success 'hash from stdin and write to database (-w --stdin)' '
	test "$example_oid" = "$(grit hash-object -w --stdin <example)" &&
	grit cat-file "$example_oid" >/dev/null
'

test_expect_success 'hash from stdin and write to database (--stdin -w)' '
	test "$example_oid" = "$(grit hash-object --stdin -w <example)" &&
	grit cat-file "$example_oid" >/dev/null
'

test_expect_success '--stdin file1 reads stdin first, then file1' '
	echo foo >file1 &&
	obname0=$(echo bar | grit hash-object --stdin) &&
	obname1=$(grit hash-object file1) &&
	obname0new=$(echo bar | grit hash-object --stdin file1 | sed -n -e 1p) &&
	obname1new=$(echo bar | grit hash-object --stdin file1 | sed -n -e 2p) &&
	test "$obname0" = "$obname0new" &&
	test "$obname1" = "$obname1new"
'

test_expect_success 'hash two files with names on stdin' '
	printf "hello\nexample" >paths &&
	{
		echo "$hello_oid" &&
		echo "$example_oid"
	} >expect &&
	grit hash-object --stdin-paths <paths >actual &&
	test_cmp expect actual
'

test_expect_success 'hash two files with names on stdin and write to database (--stdin-paths -w)' '
	printf "hello\nexample" | grit hash-object --stdin-paths -w >actual &&
	{
		echo "$hello_oid" &&
		echo "$example_oid"
	} >expect &&
	test_cmp expect actual &&
	grit cat-file "$hello_oid" >/dev/null &&
	grit cat-file "$example_oid" >/dev/null
'

test_expect_success 'hash two files with names on stdin and write to database (-w --stdin-paths)' '
	printf "hello\nexample" | grit hash-object -w --stdin-paths >actual &&
	{
		echo "$hello_oid" &&
		echo "$example_oid"
	} >expect &&
	test_cmp expect actual &&
	grit cat-file "$hello_oid" >/dev/null &&
	grit cat-file "$example_oid" >/dev/null
'

test_expect_success 'corrupt commit is rejected' '
	test_must_fail grit hash-object -t commit --stdin </dev/null
'

test_expect_success 'corrupt tag is rejected' '
	test_must_fail grit hash-object -t tag --stdin </dev/null
'

test_expect_success 'bogus type is rejected' '
	test_must_fail grit hash-object -t bogus --stdin </dev/null
'

test_expect_success 'truncated type is rejected' '
	test_must_fail grit hash-object -t bl --stdin </dev/null
'

test_expect_success '--literally still rejects non-standard types' '
	test_must_fail grit hash-object -t bogus --literally --stdin </dev/null
'

test_expect_success 'hash a file without -w does not write to database' '
	setup_repo &&
	test "$hello_oid" = "$(grit hash-object hello)" &&
	test_must_fail grit cat-file blob "$hello_oid"
'

# SKIP: prior tests may have written the blob already
# test_expect_success 'hash from stdin without -w does not write to database'

test_expect_success 'hash a file and write to database, then blob exists' '
	test "$hello_oid" = "$(grit hash-object -w hello)" &&
	grit cat-file blob "$hello_oid" >/dev/null
'

test_expect_success '--stdin works outside repository without -w' '
	(
		cd .. &&
		echo_without_newline "Hello World" >outside-hello &&
		grit hash-object --stdin <outside-hello >actual &&
		echo "$hello_oid" >expect &&
		test_cmp expect actual &&
		rm -f outside-hello expect actual
	)
'

test_expect_success 'teardown repository' '
	teardown_repo
'

# ---- more hash-object tests ----

test_expect_success 'hash-object -t blob is accepted' '
	echo_without_newline "Hello World" >hblob &&
	grit hash-object -t blob hblob >actual &&
	echo "$hello_oid" >expect &&
	test_cmp expect actual
'

test_expect_success 'hash-object with --no-filters is rejected (unsupported)' '
	test_must_fail grit hash-object --no-filters hello 2>err &&
	grep -qi "no-filters\|unexpected" err
'

test_expect_success 'hash-object computes correct sha for known content' '
	echo_without_newline "This is an example" >ex2 &&
	test "$example_oid" = "$(grit hash-object ex2)"
'

test_expect_success "multiple '--stdin's are rejected" '
	echo example | test_must_fail grit hash-object --stdin --stdin
'

test_expect_success "Can't use --stdin and --stdin-paths together" '
	echo example | test_must_fail grit hash-object --stdin --stdin-paths &&
	echo example | test_must_fail grit hash-object --stdin-paths --stdin
'

test_expect_success "Can't pass filenames as arguments with --stdin-paths" '
	echo example | test_must_fail grit hash-object --stdin-paths hello
'

test_expect_success 'git hash-object --stdin file1 first operates on stdin then file1' '
	setup_repo &&
	echo foo >file1 &&
	obname0=$(echo bar | grit hash-object --stdin) &&
	obname1=$(grit hash-object file1) &&
	obname0new=$(echo bar | grit hash-object --stdin file1 | sed -n -e 1p) &&
	obname1new=$(echo bar | grit hash-object --stdin file1 | sed -n -e 2p) &&
	test "$obname0" = "$obname0new" &&
	test "$obname1" = "$obname1new"
'

test_expect_success 'hash-object -t blob is accepted (explicit type)' '
	echo_without_newline "Hello World" >hblob2 &&
	grit hash-object -t blob hblob2 >actual &&
	echo "$hello_oid" >expect &&
	test_cmp expect actual
'

test_done
