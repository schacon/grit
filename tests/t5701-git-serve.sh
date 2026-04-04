#!/bin/sh
#
# Upstream: t5701-git-serve.sh
# Tests for protocol v2 server commands.
# All tests require 'test-tool serve-v2' and 'test-tool pkt-line'
# (C test helpers) which grit does not provide.
#

test_description='test protocol v2 server commands'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh

test_expect_success 'setup to generate files with expected content' '
	git init &&
	printf "agent=git/%s" "$(git version | cut -d" " -f3)" >agent_capability &&
	cat >expect.base <<-EOF &&
	version 2
	$(cat agent_capability)
	ls-refs=unborn
	fetch=shallow wait-for-done
	server-option
	object-format=sha1
	EOF
	cat >expect.trailer <<-EOF
	0000
	EOF
'

test_expect_success 'test capability advertisement' '
	cat expect.base expect.trailer >expect &&
	GIT_TEST_SIDEBAND_ALL=0 test-tool serve-v2 \
		--advertise-capabilities >out &&
	test-tool pkt-line unpack <out >actual &&
	test_cmp expect actual
'

test_expect_success 'stateless-rpc flag does not list capabilities' '
	test-tool pkt-line pack >in <<-EOF &&
	0000
	EOF
	test-tool serve-v2 --stateless-rpc >out <in &&
	test_must_be_empty out &&
	test-tool serve-v2 --stateless-rpc >out &&
	test_must_be_empty out
'

test_expect_success 'request invalid capability' '
	test-tool pkt-line pack >in <<-EOF &&
	foobar
	0000
	EOF
	test_must_fail test-tool serve-v2 --stateless-rpc 2>err <in &&
	test_grep "unknown capability" err
'

test_expect_success 'request with no command' '
	test-tool pkt-line pack >in <<-EOF &&
	agent=git/test
	object-format=sha1
	0000
	EOF
	test_must_fail test-tool serve-v2 --stateless-rpc 2>err <in &&
	test_grep "no command requested" err
'

test_expect_success 'request invalid command' '
	test-tool pkt-line pack >in <<-EOF &&
	command=foo
	object-format=sha1
	agent=git/test
	0000
	EOF
	test_must_fail test-tool serve-v2 --stateless-rpc 2>err <in &&
	test_grep "invalid command" err
'

test_expect_success 'request capability as command' '
	test-tool pkt-line pack >in <<-EOF &&
	command=agent
	object-format=sha1
	0000
	EOF
	test_must_fail test-tool serve-v2 --stateless-rpc 2>err <in &&
	grep invalid.command.*agent err
'

test_expect_success 'request command as capability' '
	test-tool pkt-line pack >in <<-EOF &&
	command=ls-refs
	object-format=sha1
	fetch
	0000
	EOF
	test_must_fail test-tool serve-v2 --stateless-rpc 2>err <in &&
	grep unknown.capability err
'

test_expect_success 'requested command is command=value' '
	test-tool pkt-line pack >in <<-EOF &&
	command=ls-refs=whatever
	object-format=sha1
	0000
	EOF
	test_must_fail test-tool serve-v2 --stateless-rpc 2>err <in &&
	grep invalid.command.*ls-refs=whatever err
'

test_expect_success 'wrong object-format' '
	test-tool pkt-line pack >in <<-EOF &&
	command=fetch
	agent=git/test
	object-format=sha256
	0000
	EOF
	test_must_fail test-tool serve-v2 --stateless-rpc 2>err <in &&
	test_grep "mismatched object format" err
'

test_expect_success 'setup some refs and tags' '
	test_commit one &&
	git branch dev main &&
	test_commit two &&
	git symbolic-ref refs/heads/release refs/heads/main &&
	git tag -a -m "annotated tag" annotated-tag
'

test_expect_success 'basics of ls-refs' '
	test-tool pkt-line pack >in <<-EOF &&
	command=ls-refs
	object-format=sha1
	0000
	EOF

	cat >expect <<-EOF &&
	$(git rev-parse HEAD) HEAD
	$(git rev-parse refs/heads/dev) refs/heads/dev
	$(git rev-parse refs/heads/main) refs/heads/main
	$(git rev-parse refs/heads/release) refs/heads/release
	$(git rev-parse refs/tags/annotated-tag) refs/tags/annotated-tag
	$(git rev-parse refs/tags/one) refs/tags/one
	$(git rev-parse refs/tags/two) refs/tags/two
	0000
	EOF

	test-tool serve-v2 --stateless-rpc <in >out &&
	test-tool pkt-line unpack <out >actual &&
	test_cmp expect actual
'

test_expect_success 'ls-refs complains about unknown options' '
	test-tool pkt-line pack >in <<-EOF &&
	command=ls-refs
	object-format=sha1
	0001
	no-such-arg
	0000
	EOF

	test_must_fail test-tool serve-v2 --stateless-rpc 2>err <in &&
	grep unexpected.line.*no-such-arg err
'

test_expect_success 'basic ref-prefixes' '
	test-tool pkt-line pack >in <<-EOF &&
	command=ls-refs
	object-format=sha1
	0001
	ref-prefix refs/heads/main
	ref-prefix refs/tags/one
	0000
	EOF

	cat >expect <<-EOF &&
	$(git rev-parse refs/heads/main) refs/heads/main
	$(git rev-parse refs/tags/one) refs/tags/one
	0000
	EOF

	test-tool serve-v2 --stateless-rpc <in >out &&
	test-tool pkt-line unpack <out >actual &&
	test_cmp expect actual
'

test_expect_success 'refs/heads prefix' '
	test-tool pkt-line pack >in <<-EOF &&
	command=ls-refs
	object-format=sha1
	0001
	ref-prefix refs/heads/
	0000
	EOF

	cat >expect <<-EOF &&
	$(git rev-parse refs/heads/dev) refs/heads/dev
	$(git rev-parse refs/heads/main) refs/heads/main
	$(git rev-parse refs/heads/release) refs/heads/release
	0000
	EOF

	test-tool serve-v2 --stateless-rpc <in >out &&
	test-tool pkt-line unpack <out >actual &&
	test_cmp expect actual
'

test_expect_success 'ignore very large set of prefixes' '
	{
		echo command=ls-refs &&
		echo object-format=sha1 &&
		echo 0001 &&
		awk "BEGIN { for (i = 1; i <= 65536; i++) print \"ref-prefix refs/heads/\" i }" &&
		echo 0000
	} |
	test-tool pkt-line pack >in &&

	cat >expect <<-EOF &&
	$(git rev-parse HEAD) HEAD
	$(git rev-parse refs/heads/dev) refs/heads/dev
	$(git rev-parse refs/heads/main) refs/heads/main
	$(git rev-parse refs/heads/release) refs/heads/release
	$(git rev-parse refs/tags/annotated-tag) refs/tags/annotated-tag
	$(git rev-parse refs/tags/one) refs/tags/one
	$(git rev-parse refs/tags/two) refs/tags/two
	0000
	EOF

	test-tool serve-v2 --stateless-rpc <in >out &&
	test-tool pkt-line unpack <out >actual &&
	test_cmp expect actual
'

test_expect_success 'peel parameter' '
	test-tool pkt-line pack >in <<-EOF &&
	command=ls-refs
	object-format=sha1
	0001
	peel
	ref-prefix refs/tags/
	0000
	EOF

	cat >expect <<-EOF &&
	$(git rev-parse refs/tags/annotated-tag) refs/tags/annotated-tag peeled:$(git rev-parse refs/tags/annotated-tag^{})
	$(git rev-parse refs/tags/one) refs/tags/one
	$(git rev-parse refs/tags/two) refs/tags/two
	0000
	EOF

	test-tool serve-v2 --stateless-rpc <in >out &&
	test-tool pkt-line unpack <out >actual &&
	test_cmp expect actual
'

test_expect_success 'symrefs parameter' '
	test-tool pkt-line pack >in <<-EOF &&
	command=ls-refs
	object-format=sha1
	0001
	symrefs
	ref-prefix refs/heads/
	0000
	EOF

	cat >expect <<-EOF &&
	$(git rev-parse refs/heads/dev) refs/heads/dev
	$(git rev-parse refs/heads/main) refs/heads/main
	$(git rev-parse refs/heads/release) refs/heads/release symref-target:refs/heads/main
	0000
	EOF

	test-tool serve-v2 --stateless-rpc <in >out &&
	test-tool pkt-line unpack <out >actual &&
	test_cmp expect actual
'

test_expect_success 'sending server-options' '
	test-tool pkt-line pack >in <<-EOF &&
	command=ls-refs
	object-format=sha1
	server-option=hello
	server-option=world
	0001
	ref-prefix HEAD
	0000
	EOF

	cat >expect <<-EOF &&
	$(git rev-parse HEAD) HEAD
	0000
	EOF

	test-tool serve-v2 --stateless-rpc <in >out &&
	test-tool pkt-line unpack <out >actual &&
	test_cmp expect actual
'

test_expect_success 'unexpected lines are not allowed in fetch request' '
	git init server &&

	test-tool pkt-line pack >in <<-EOF &&
	command=fetch
	object-format=sha1
	0001
	this-is-not-a-command
	0000
	EOF

	(
		cd server &&
		test_must_fail test-tool serve-v2 --stateless-rpc
	) <in >/dev/null 2>err &&
	grep "unexpected line: .this-is-not-a-command." err
'

test_expect_success 'basics of object-info' '
	test_config transfer.advertiseObjectInfo true &&

	test-tool pkt-line pack >in <<-EOF &&
	command=object-info
	object-format=sha1
	0001
	size
	oid $(git rev-parse two:two.t)
	oid $(git rev-parse two:two.t)
	0000
	EOF

	cat >expect <<-EOF &&
	size
	$(git rev-parse two:two.t) $(wc -c <two.t | xargs)
	$(git rev-parse two:two.t) $(wc -c <two.t | xargs)
	0000
	EOF

	test-tool serve-v2 --stateless-rpc <in >out &&
	test-tool pkt-line unpack <out >actual &&
	test_cmp expect actual
'

test_expect_success 'test capability advertisement with uploadpack.advertiseBundleURIs' '
	test_config uploadpack.advertiseBundleURIs true &&

	cat >expect.extra <<-EOF &&
	bundle-uri
	EOF
	cat expect.base \
	    expect.extra \
	    expect.trailer >expect &&

	GIT_TEST_SIDEBAND_ALL=0 test-tool serve-v2 \
		--advertise-capabilities >out &&
	test-tool pkt-line unpack <out >actual &&
	test_cmp expect actual
'

test_expect_success 'basics of bundle-uri: dies if not enabled' '
	test-tool pkt-line pack >in <<-EOF &&
	command=bundle-uri
	0000
	EOF

	test_must_fail test-tool serve-v2 --stateless-rpc <in >out 2>err.actual &&
	test_must_be_empty out
'

test_expect_success 'object-info missing from capabilities when disabled' '
	test_config transfer.advertiseObjectInfo false &&

	GIT_TEST_SIDEBAND_ALL=0 test-tool serve-v2 \
		--advertise-capabilities >out &&
	test-tool pkt-line unpack <out >actual &&

	! grep object.info actual
'

test_expect_success 'object-info commands rejected when disabled' '
	test_config transfer.advertiseObjectInfo false &&

	test-tool pkt-line pack >in <<-EOF &&
	command=object-info
	EOF

	test_must_fail test-tool serve-v2 --stateless-rpc <in 2>err &&
	grep invalid.command err
'

test_done
