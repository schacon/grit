#!/bin/sh
#
# Upstream: t5750-bundle-uri-parse.sh
# Tests for bundle-uri configuration parsing.
# All tests require 'test-tool bundle-uri' (a C test helper) which grit
# does not provide, so all are test_expect_success with real bodies.
#

test_description='bundle-uri parse tests'

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

# Helper used by upstream — normalize config output for comparison
test_cmp_config_output () {
	test_cmp "$1" "$2"
}

test_expect_failure 'bundle_uri_parse_line() just URIs' '
	cat >in <<-\EOF &&
	bundle.one.uri=http://example.com/bundle.bdl
	bundle.two.uri=https://example.com/bundle.bdl
	bundle.three.uri=file:///usr/share/git/bundle.bdl
	EOF

	cat >expect <<-\EOF &&
	[bundle]
		version = 1
		mode = all
	[bundle "one"]
		uri = http://example.com/bundle.bdl
	[bundle "two"]
		uri = https://example.com/bundle.bdl
	[bundle "three"]
		uri = file:///usr/share/git/bundle.bdl
	EOF

	test-tool bundle-uri parse-key-values in >actual 2>err &&
	test_must_be_empty err &&
	test_cmp_config_output expect actual
'

test_expect_failure 'bundle_uri_parse_line(): relative URIs' '
	cat >in <<-\EOF &&
	bundle.one.uri=bundle.bdl
	bundle.two.uri=../bundle.bdl
	bundle.three.uri=sub/dir/bundle.bdl
	EOF

	cat >expect <<-\EOF &&
	[bundle]
		version = 1
		mode = all
	[bundle "one"]
		uri = <uri>/bundle.bdl
	[bundle "two"]
		uri = bundle.bdl
	[bundle "three"]
		uri = <uri>/sub/dir/bundle.bdl
	EOF

	test-tool bundle-uri parse-key-values in >actual 2>err &&
	test_must_be_empty err &&
	test_cmp_config_output expect actual
'

test_expect_failure 'bundle_uri_parse_line(): relative URIs and parent paths' '
	cat >in <<-\EOF &&
	bundle.one.uri=bundle.bdl
	bundle.two.uri=../bundle.bdl
	bundle.three.uri=../../bundle.bdl
	EOF

	test_must_fail test-tool bundle-uri parse-key-values in >actual 2>err &&
	grep "fatal: cannot strip one component off url" err
'

test_expect_failure 'bundle_uri_parse_line() parsing edge cases: empty key or value' '
	cat >in <<-\EOF &&
	=bogus-value
	bogus-key=
	EOF

	cat >expect <<-\EOF &&
	[bundle]
		version = 1
		mode = all
	EOF

	test_must_fail test-tool bundle-uri parse-key-values in >actual 2>err &&
	test_cmp_config_output expect actual
'

test_expect_failure 'bundle_uri_parse_line() parsing edge cases: empty lines' '
	cat >in <<-\EOF &&
	bundle.one.uri=http://example.com/bundle.bdl

	bundle.two.uri=https://example.com/bundle.bdl

	bundle.three.uri=file:///usr/share/git/bundle.bdl
	EOF

	cat >expect <<-\EOF &&
	[bundle]
		version = 1
		mode = all
	[bundle "one"]
		uri = http://example.com/bundle.bdl
	[bundle "two"]
		uri = https://example.com/bundle.bdl
	[bundle "three"]
		uri = file:///usr/share/git/bundle.bdl
	EOF

	test_must_fail test-tool bundle-uri parse-key-values in >actual 2>err &&
	test_cmp_config_output expect actual
'

test_expect_failure 'bundle_uri_parse_line() parsing edge cases: duplicate lines' '
	cat >in <<-\EOF &&
	bundle.one.uri=http://example.com/bundle.bdl
	bundle.two.uri=https://example.com/bundle.bdl
	bundle.one.uri=https://example.com/bundle-2.bdl
	bundle.three.uri=file:///usr/share/git/bundle.bdl
	EOF

	cat >expect <<-\EOF &&
	[bundle]
		version = 1
		mode = all
	[bundle "one"]
		uri = http://example.com/bundle.bdl
	[bundle "two"]
		uri = https://example.com/bundle.bdl
	[bundle "three"]
		uri = file:///usr/share/git/bundle.bdl
	EOF

	test_must_fail test-tool bundle-uri parse-key-values in >actual 2>err &&
	test_cmp_config_output expect actual
'

test_expect_failure 'parse config format: just URIs' '
	cat >expect <<-\EOF &&
	[bundle]
		version = 1
		mode = all
	[bundle "one"]
		uri = http://example.com/bundle.bdl
	[bundle "two"]
		uri = https://example.com/bundle.bdl
	[bundle "three"]
		uri = file:///usr/share/git/bundle.bdl
	EOF

	test-tool bundle-uri parse-config expect >actual 2>err &&
	test_must_be_empty err &&
	test_cmp_config_output expect actual
'

test_expect_failure 'parse config format: relative URIs' '
	cat >in <<-\EOF &&
	[bundle]
		version = 1
		mode = all
	[bundle "one"]
		uri = bundle.bdl
	[bundle "two"]
		uri = ../bundle.bdl
	[bundle "three"]
		uri = sub/dir/bundle.bdl
	EOF

	cat >expect <<-\EOF &&
	[bundle]
		version = 1
		mode = all
	[bundle "one"]
		uri = <uri>/bundle.bdl
	[bundle "two"]
		uri = bundle.bdl
	[bundle "three"]
		uri = <uri>/sub/dir/bundle.bdl
	EOF

	test-tool bundle-uri parse-config in >actual 2>err &&
	test_must_be_empty err &&
	test_cmp_config_output expect actual
'

test_expect_failure 'parse config format edge cases: empty key or value' '
	cat >in1 <<-\EOF &&
	= bogus-value
	EOF

	cat >expect <<-\EOF &&
	[bundle]
		version = 1
		mode = all
	EOF

	test_must_fail test-tool bundle-uri parse-config in1 >actual 2>err &&
	test_cmp_config_output expect actual
'

test_expect_failure 'parse config format: creationToken heuristic' '
	cat >expect <<-\EOF &&
	[bundle]
		version = 1
		mode = all
		heuristic = creationToken
	[bundle "one"]
		uri = http://example.com/bundle.bdl
		creationToken = 123456
	[bundle "two"]
		uri = https://example.com/bundle.bdl
		creationToken = 12345678901234567890
	[bundle "three"]
		uri = file:///usr/share/git/bundle.bdl
		creationToken = 1
	EOF

	test-tool bundle-uri parse-config expect >actual 2>err &&
	test_must_be_empty err &&
	test_cmp_config_output expect actual
'

test_expect_failure 'parse config format edge cases: creationToken heuristic' '
	cat >expect <<-\EOF &&
	[bundle]
		version = 1
		mode = all
		heuristic = creationToken
	[bundle "one"]
		uri = http://example.com/bundle.bdl
		creationToken = bogus
	EOF

	test-tool bundle-uri parse-config expect >actual 2>err &&
	grep "could not parse bundle list key creationToken" err
'

test_expect_failure 'parse config format: bundle with missing uri' '
	cat >input <<-\EOF &&
	[bundle]
		version = 1
		mode = all
	[bundle "missing-uri"]
		creationToken = 1
	EOF

	test_must_fail test-tool bundle-uri parse-config input 2>err &&
	grep "bundle .missing-uri. has no uri" err
'

test_expect_failure 'parse config format: bundle with url instead of uri' '
	cat >input <<-\EOF &&
	[bundle]
		version = 1
		mode = all
	[bundle "typo"]
		url = https://example.com/bundle.bdl
	EOF

	test_must_fail test-tool bundle-uri parse-config input 2>err &&
	grep "bundle .typo. has no uri" err
'

test_done
