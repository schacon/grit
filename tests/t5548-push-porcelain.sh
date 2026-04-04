#!/bin/sh
# Ported from git/t/t5548-push-porcelain.sh
#
# Copyright (c) 2020 Jiang Xin
#
# Only the file-protocol tests are ported; HTTP tests are omitted.

test_description='Test git push porcelain output'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

. ./test-lib.sh

create_commits_in () {
	repo="$1" && test -d "$repo" ||
	error "Repository $repo does not exist."
	shift &&
	while test $# -gt 0
	do
		name=$1 &&
		shift &&
		test_tick &&
		echo "$name" >"$repo/$name.t" &&
		git -C "$repo" add "$name.t" &&
		git -C "$repo" commit -q -m "$name" &&
		eval $name=$(git -C "$repo" rev-parse HEAD)
	done
}

get_abbrev_oid () {
	oid=$1 &&
	suffix=${oid#???????} &&
	oid=${oid%$suffix} &&
	if test -n "$oid"
	then
		echo "$oid"
	else
		echo "undefined-oid"
	fi
}

make_user_friendly_and_stable_output () {
	sed \
		-e "s/$(get_abbrev_oid $A)[0-9a-f]*/<COMMIT-A>/g" \
		-e "s/$(get_abbrev_oid $B)[0-9a-f]*/<COMMIT-B>/g" \
		-e "s/$ZERO_OID/<ZERO-OID>/g" \
		-e "s#To $URL_PREFIX/upstream.git#To <URL/of/upstream.git>#"
}

format_and_save_expect () {
	sed -e 's/^> //' -e 's/Z$//' >expect
}

create_upstream_template () {
	git init --bare upstream-template.git &&
	git clone upstream-template.git tmp_work_dir &&
	create_commits_in tmp_work_dir A B &&
	(
		cd tmp_work_dir &&
		git push origin \
			$B:refs/heads/main \
			$A:refs/heads/foo \
			$A:refs/heads/bar \
			$A:refs/heads/baz
	) &&
	rm -rf tmp_work_dir
}

setup_upstream () {
	if test $# -ne 1
	then
		echo "BUG: location of upstream repository is not provided"
		return 1
	fi &&
	rm -rf "$1" &&
	if ! test -d upstream-template.git
	then
		create_upstream_template
	fi &&
	git clone --mirror upstream-template.git "$1"
}

setup_upstream_and_workbench () {
	if test $# -ne 1
	then
		echo "BUG: location of upstream repository is not provided"
		return 1
	fi
	upstream="$1"

	test_expect_failure "setup upstream repository and workbench (grit: clone of bare repo HEAD resolution)" '
		setup_upstream "$upstream" &&
		rm -rf workbench &&
		git clone "$upstream" workbench &&
		(
			cd workbench &&
			git update-ref refs/heads/main $A &&
			git update-ref refs/heads/baz $A &&
			git update-ref refs/heads/next $A &&
			git config core.abbrev 7 &&
			git config advice.pushUpdateRejected false
		)
	'
}

run_git_push_porcelain_output_test() {
	case $1 in
	file)
		PROTOCOL="builtin protocol"
		URL_PREFIX=".*"
		;;
	esac

	test_expect_failure ".. git-push --porcelain ($PROTOCOL) (grit: porcelain output format)" '
		test_when_finished "setup_upstream \"$upstream\"" &&
		test_must_fail git -C workbench push --porcelain origin \
			main \
			:refs/heads/foo \
			$B:bar \
			baz \
			next >out &&
		make_user_friendly_and_stable_output <out >actual &&
		format_and_save_expect <<-\EOF &&
		> To <URL/of/upstream.git>
		> =	refs/heads/baz:refs/heads/baz	[up to date]
		>  	<COMMIT-B>:refs/heads/bar	<COMMIT-A>..<COMMIT-B>
		> -	:refs/heads/foo	[deleted]
		> *	refs/heads/next:refs/heads/next	[new branch]
		> !	refs/heads/main:refs/heads/main	[rejected] (non-fast-forward)
		> Done
		EOF
		test_cmp expect actual
	'

	test_expect_failure ".. git-push --porcelain --force ($PROTOCOL) (grit: porcelain output format)" '
		test_when_finished "setup_upstream \"$upstream\"" &&
		git -C workbench push --porcelain --force origin \
			main \
			:refs/heads/foo \
			$B:bar \
			baz \
			next >out &&
		make_user_friendly_and_stable_output <out >actual &&
		format_and_save_expect <<-EOF &&
		> To <URL/of/upstream.git>
		> =	refs/heads/baz:refs/heads/baz	[up to date]
		>  	<COMMIT-B>:refs/heads/bar	<COMMIT-A>..<COMMIT-B>
		> -	:refs/heads/foo	[deleted]
		> +	refs/heads/main:refs/heads/main	<COMMIT-B>...<COMMIT-A> (forced update)
		> *	refs/heads/next:refs/heads/next	[new branch]
		> Done
		EOF
		test_cmp expect actual
	'

	test_expect_failure ".. git push --porcelain --atomic ($PROTOCOL) (grit: porcelain output format)" '
		test_when_finished "setup_upstream \"$upstream\"" &&
		test_must_fail git -C workbench push --porcelain --atomic origin \
			main \
			:refs/heads/foo \
			$B:bar \
			baz \
			next >out &&
		make_user_friendly_and_stable_output <out >actual &&
		format_and_save_expect <<-EOF &&
		> To <URL/of/upstream.git>
		> =	refs/heads/baz:refs/heads/baz	[up to date]
		> !	<COMMIT-B>:refs/heads/bar	[rejected] (atomic push failed)
		> !	(delete):refs/heads/foo	[rejected] (atomic push failed)
		> !	refs/heads/main:refs/heads/main	[rejected] (non-fast-forward)
		> !	refs/heads/next:refs/heads/next	[rejected] (atomic push failed)
		> Done
		EOF
		test_cmp expect actual
	'

	test_expect_failure ".. pre-receive hook declined ($PROTOCOL) (grit: porcelain output format)" '
		test_when_finished "rm -f \"$upstream/hooks/pre-receive\" &&
			setup_upstream \"$upstream\"" &&
		test_hook --setup -C "$upstream" pre-receive <<-EOF &&
			exit 1
		EOF
		test_must_fail git -C workbench push --porcelain --force origin \
			main \
			:refs/heads/foo \
			$B:bar \
			baz \
			next >out &&
		make_user_friendly_and_stable_output <out >actual &&
		format_and_save_expect <<-EOF &&
		> To <URL/of/upstream.git>
		> =	refs/heads/baz:refs/heads/baz	[up to date]
		> !	<COMMIT-B>:refs/heads/bar	[remote rejected] (pre-receive hook declined)
		> !	:refs/heads/foo	[remote rejected] (pre-receive hook declined)
		> !	refs/heads/main:refs/heads/main	[remote rejected] (pre-receive hook declined)
		> !	refs/heads/next:refs/heads/next	[remote rejected] (pre-receive hook declined)
		> Done
		EOF
		test_cmp expect actual
	'

	test_expect_failure ".. non-fastforward push ($PROTOCOL) (grit: porcelain output format)" '
		test_when_finished "setup_upstream \"$upstream\"" &&
		(
			cd workbench &&
			test_must_fail git push --porcelain origin \
				main \
				next
		) >out &&
		make_user_friendly_and_stable_output <out >actual &&
		format_and_save_expect <<-EOF &&
		> To <URL/of/upstream.git>
		> *	refs/heads/next:refs/heads/next	[new branch]
		> !	refs/heads/main:refs/heads/main	[rejected] (non-fast-forward)
		> Done
		EOF
		test_cmp expect actual
	'

	test_expect_failure ".. git push --porcelain --atomic --force ($PROTOCOL) (grit: porcelain output format)" '
		git -C workbench push --porcelain --atomic --force origin \
			main \
			:refs/heads/foo \
			$B:bar \
			baz \
			next >out &&
		make_user_friendly_and_stable_output <out >actual &&
		format_and_save_expect <<-\EOF &&
		> To <URL/of/upstream.git>
		> =	refs/heads/baz:refs/heads/baz	[up to date]
		>  	<COMMIT-B>:refs/heads/bar	<COMMIT-A>..<COMMIT-B>
		> -	:refs/heads/foo	[deleted]
		> +	refs/heads/main:refs/heads/main	<COMMIT-B>...<COMMIT-A> (forced update)
		> *	refs/heads/next:refs/heads/next	[new branch]
		> Done
		EOF
		test_cmp expect actual
	'
}

run_git_push_dry_run_porcelain_output_test() {
	case $1 in
	file)
		PROTOCOL="builtin protocol"
		URL_PREFIX=".*"
		;;
	esac

	test_expect_failure ".. git-push --porcelain --dry-run ($PROTOCOL) (grit: porcelain output format)" '
		test_must_fail git -C workbench push --porcelain --dry-run origin \
			main \
			:refs/heads/foo \
			$B:bar \
			baz \
			next >out &&
		make_user_friendly_and_stable_output <out >actual &&
		format_and_save_expect <<-EOF &&
		> To <URL/of/upstream.git>
		> =	refs/heads/baz:refs/heads/baz	[up to date]
		>  	<COMMIT-B>:refs/heads/bar	<COMMIT-A>..<COMMIT-B>
		> -	:refs/heads/foo	[deleted]
		> *	refs/heads/next:refs/heads/next	[new branch]
		> !	refs/heads/main:refs/heads/main	[rejected] (non-fast-forward)
		> Done
		EOF
		test_cmp expect actual
	'

	test_expect_failure ".. git-push --porcelain --dry-run --force ($PROTOCOL) (grit: porcelain output format)" '
		git -C workbench push --porcelain --dry-run --force origin \
			main \
			:refs/heads/foo \
			$B:bar \
			baz \
			next >out &&
		make_user_friendly_and_stable_output <out >actual &&
		format_and_save_expect <<-EOF &&
		> To <URL/of/upstream.git>
		> =	refs/heads/baz:refs/heads/baz	[up to date]
		>  	<COMMIT-B>:refs/heads/bar	<COMMIT-A>..<COMMIT-B>
		> -	:refs/heads/foo	[deleted]
		> +	refs/heads/main:refs/heads/main	<COMMIT-B>...<COMMIT-A> (forced update)
		> *	refs/heads/next:refs/heads/next	[new branch]
		> Done
		EOF
		test_cmp expect actual
	'

	test_expect_failure ".. git-push --porcelain --dry-run --atomic ($PROTOCOL) (grit: porcelain output format)" '
		test_must_fail git -C workbench push --porcelain --dry-run --atomic origin \
			main \
			:refs/heads/foo \
			$B:bar \
			baz \
			next >out &&
		make_user_friendly_and_stable_output <out >actual &&
		format_and_save_expect <<-EOF &&
		> To <URL/of/upstream.git>
		> =	refs/heads/baz:refs/heads/baz	[up to date]
		> !	<COMMIT-B>:refs/heads/bar	[rejected] (atomic push failed)
		> !	(delete):refs/heads/foo	[rejected] (atomic push failed)
		> !	refs/heads/main:refs/heads/main	[rejected] (non-fast-forward)
		> !	refs/heads/next:refs/heads/next	[rejected] (atomic push failed)
		> Done
		EOF
		test_cmp expect actual
	'

	test_expect_failure ".. git-push --porcelain --dry-run --atomic --force ($PROTOCOL) (grit: porcelain output format)" '
		git -C workbench push --porcelain --dry-run --atomic --force origin \
			main \
			:refs/heads/foo \
			$B:bar \
			baz \
			next >out &&
		make_user_friendly_and_stable_output <out >actual &&
		format_and_save_expect <<-EOF &&
		> To <URL/of/upstream.git>
		> =	refs/heads/baz:refs/heads/baz	[up to date]
		>  	<COMMIT-B>:refs/heads/bar	<COMMIT-A>..<COMMIT-B>
		> -	:refs/heads/foo	[deleted]
		> +	refs/heads/main:refs/heads/main	<COMMIT-B>...<COMMIT-A> (forced update)
		> *	refs/heads/next:refs/heads/next	[new branch]
		> Done
		EOF
		test_cmp expect actual
	'
}

setup_upstream_and_workbench upstream.git

run_git_push_porcelain_output_test file

setup_upstream_and_workbench upstream.git

run_git_push_dry_run_porcelain_output_test file

# NOTE: HTTP protocol tests from upstream are omitted.

test_done
