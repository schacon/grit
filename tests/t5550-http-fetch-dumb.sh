#!/bin/sh
#
# Upstream: t5550-http-fetch-dumb.sh
# Tests dumb HTTP fetching over static files served by test-httpd.
#

test_description='test dumb fetching over http via static file'

GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME=main
export GIT_TEST_DEFAULT_INITIAL_BRANCH_NAME

cd "$(dirname "$0")" || exit 1
. ./test-lib.sh
. "$TEST_DIRECTORY"/lib-httpd.sh
start_httpd

test_expect_success 'setup repository' '
	git config push.default matching &&
	echo content1 >file &&
	git add file &&
	git commit -m one &&
	echo content2 >file &&
	git add file &&
	git commit -m two
'

setup_post_update_server_info_hook () {
	test_hook --setup -C "$1" post-update <<-\EOF &&
	exec git update-server-info
	EOF
	git -C "$1" update-server-info
}

test_expect_success 'create http-accessible bare repository with loose objects' '
	cp -R .git "$HTTPD_DOCUMENT_ROOT_PATH/repo.git" &&
	git -C "$HTTPD_DOCUMENT_ROOT_PATH/repo.git" config core.bare true &&
	setup_post_update_server_info_hook "$HTTPD_DOCUMENT_ROOT_PATH/repo.git" &&
	git remote add public "$HTTPD_DOCUMENT_ROOT_PATH/repo.git" &&
	git push public main:main
'

test_expect_success 'clone http repository' '
	git clone $HTTPD_URL/dumb/repo.git clone-tmpl &&
	cp -R clone-tmpl clone &&
	test_cmp file clone/file
'

test_expect_success 'list refs from outside any repository' '
	cat >expect <<-EOF &&
	$(git rev-parse main)	HEAD
	$(git rev-parse main)	refs/heads/main
	EOF
	nongit git ls-remote "$HTTPD_URL/dumb/repo.git" >actual &&
	test_cmp expect actual
'

test_expect_success 'create password-protected repository' '
	mkdir -p "$HTTPD_DOCUMENT_ROOT_PATH/auth/dumb/" &&
	cp -Rf "$HTTPD_DOCUMENT_ROOT_PATH/repo.git" \
	       "$HTTPD_DOCUMENT_ROOT_PATH/auth/dumb/repo.git"
'

test_expect_success 'create empty remote repository' '
	git init --bare "$HTTPD_DOCUMENT_ROOT_PATH/empty.git" &&
	setup_post_update_server_info_hook "$HTTPD_DOCUMENT_ROOT_PATH/empty.git"
'

test_expect_success 'empty dumb HTTP repository falls back to SHA1' '
	rm -fr clone-empty &&
	git clone $HTTPD_URL/dumb/empty.git clone-empty &&
	git -C clone-empty rev-parse --show-object-format >empty-format &&
	test "$(cat empty-format)" = sha1
'

setup_askpass_helper

test_expect_success 'cloning password-protected repository can fail' '
	set_askpass wrong &&
	test_must_fail git clone "$HTTPD_URL/auth/dumb/repo.git" clone-auth-fail &&
	expect_askpass both wrong
'

test_expect_success 'http auth can use user/pass in URL' '
	set_askpass wrong &&
	git clone "$HTTPD_URL_USER_PASS/auth/dumb/repo.git" clone-auth-none &&
	expect_askpass none
'

test_expect_success 'fetch changes via http' '
	echo content >>file &&
	git commit -a -m three &&
	git push public &&
	(cd clone && git pull) &&
	test_cmp file clone/file
'

test_expect_success 'fetch packed objects' '
	cp -R "$HTTPD_DOCUMENT_ROOT_PATH"/repo.git "$HTTPD_DOCUMENT_ROOT_PATH"/repo_pack.git &&
	(cd "$HTTPD_DOCUMENT_ROOT_PATH"/repo_pack.git &&
	 git --bare repack -a -d
	) &&
	git clone $HTTPD_URL/dumb/repo_pack.git
'

test_expect_success 'did not use upload-pack service' '
	test_might_fail kill $(cat "$HTTPD_ROOT_PATH/httpd.pid") &&
	start_httpd
'

test_done
