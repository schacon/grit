# Shell library to run an HTTP server for use in tests.
#
# Replaces upstream's Apache-based lib-httpd.sh with a lightweight
# Rust HTTP server (test-httpd binary).
#
# Usage:
#
#   . ./test-lib.sh
#   . "$TEST_DIRECTORY"/lib-httpd.sh
#   start_httpd
#
#   test_expect_success '...' '
#       ...
#   '
#
#   test_done
#
# Variables:
#   LIB_HTTPD_PORT    — port (default: 0 = random)
#   HTTPD_URL         — set after start_httpd (e.g. http://127.0.0.1:PORT)
#   HTTPD_DOCUMENT_ROOT_PATH — document root for serving files

# HTTP transport tests need real git for client operations since grit
# doesn't support HTTP transport yet. Override the wrapper to use real git.
REAL_GIT="$(command -v git 2>/dev/null || echo /usr/bin/git)"
# Strip our .bin wrapper from PATH to find the real git
for _p in $(echo "$PATH" | tr ':' ' '); do
	if test -x "$_p/git" && ! grep -q 'GUST_BIN\|grit' "$_p/git" 2>/dev/null; then
		REAL_GIT="$_p/git"
		break
	fi
done

# Replace the git wrapper with real git for HTTP transport
if test -n "$TRASH_DIRECTORY" && test -d "$TRASH_DIRECTORY/.bin"; then
	cat >"$TRASH_DIRECTORY/.bin/git" <<EOFWRAP
#!/bin/sh
exec "$REAL_GIT" "\$@"
EOFWRAP
	chmod +x "$TRASH_DIRECTORY/.bin/git"
fi

# Find the test-httpd binary
REPO_ROOT="$(cd "$TEST_DIRECTORY/.." && pwd)"
TEST_HTTPD_BIN="$REPO_ROOT/target/debug/test-httpd"

if ! test -x "$TEST_HTTPD_BIN"
then
	# Try release build
	TEST_HTTPD_BIN="$REPO_ROOT/target/release/test-httpd"
fi

if ! test -x "$TEST_HTTPD_BIN"
then
	skip_all='test-httpd binary not found; build with: cargo build -p grit-rs'
	test_done
fi

# Set up paths
HTTPD_ROOT_PATH="$PWD/httpd"
HTTPD_DOCUMENT_ROOT_PATH="$HTTPD_ROOT_PATH/www"

# Default auth credentials (matching upstream's passwd file)
HTTPD_AUTH_USER="user@host"
HTTPD_AUTH_PASS="pass@host"

HTTPD_PROTO=http

prepare_httpd() {
	mkdir -p "$HTTPD_DOCUMENT_ROOT_PATH"
	mkdir -p "$HTTPD_DOCUMENT_ROOT_PATH/auth/dumb"
}

start_httpd() {
	prepare_httpd

	local port_arg=""
	if test -n "$LIB_HTTPD_PORT"
	then
		port_arg="--port $LIB_HTTPD_PORT"
	fi

	# Start server in background, capture the READY line for the port
	"$TEST_HTTPD_BIN" \
		--root "$HTTPD_DOCUMENT_ROOT_PATH" \
		--auth "${HTTPD_AUTH_USER}:${HTTPD_AUTH_PASS}" \
		--pid-file "$HTTPD_ROOT_PATH/httpd.pid" \
		$port_arg \
		>"$HTTPD_ROOT_PATH/httpd.out" \
		2>"$HTTPD_ROOT_PATH/httpd.err" &
	HTTPD_PID=$!

	# Wait for READY line (up to 5 seconds)
	local tries=0
	while test $tries -lt 50
	do
		if test -s "$HTTPD_ROOT_PATH/httpd.out"
		then
			break
		fi
		sleep 0.1
		tries=$((tries + 1))
	done

	if ! test -s "$HTTPD_ROOT_PATH/httpd.out"
	then
		echo "test-httpd failed to start" >&2
		if test -s "$HTTPD_ROOT_PATH/httpd.err"
		then
			cat "$HTTPD_ROOT_PATH/httpd.err" >&2
		fi
		return 1
	fi

	LIB_HTTPD_PORT=$(sed -n 's/^READY //p' "$HTTPD_ROOT_PATH/httpd.out")
	if test -z "$LIB_HTTPD_PORT"
	then
		echo "Could not determine test-httpd port" >&2
		kill "$HTTPD_PID" 2>/dev/null
		return 1
	fi

	HTTPD_DEST="127.0.0.1:$LIB_HTTPD_PORT"
	HTTPD_URL="$HTTPD_PROTO://$HTTPD_DEST"
	HTTPD_URL_USER="$HTTPD_PROTO://user%40host@$HTTPD_DEST"
	HTTPD_URL_USER_PASS="$HTTPD_PROTO://user%40host:pass%40host@$HTTPD_DEST"

	# Register cleanup at script exit
	trap 'stop_httpd' EXIT
}

stop_httpd() {
	if test -n "$HTTPD_PID"
	then
		kill "$HTTPD_PID" 2>/dev/null || :
		sleep 0.2
		kill -9 "$HTTPD_PID" 2>/dev/null || :
		HTTPD_PID=
	fi
}

strip_access_log () {
	sed -e "s/  */ /g" <"$HTTPD_ROOT_PATH/access.log"
}

check_access_log () {
	strip_access_log >access.log.stripped &&
	if ! test -s "$1"; then test_must_be_empty access.log.stripped
	else test_cmp "$1" access.log.stripped; fi
}

test_http_push_nonff () {
	REMOTE_REPO=$1; LOCAL_REPO=$2; BRANCH=$3; EXPECT_CAS_RESULT=${4-failure}
	test_expect_success 'non-fast-forward push fails and shows status' '
		cd "$REMOTE_REPO" && HEAD=$(git rev-parse --verify HEAD) &&
		cd "$LOCAL_REPO" && git checkout $BRANCH &&
		echo "changed" > path2 && git commit -a -m path2 --amend &&
		test_must_fail git push -v origin >output 2>&1 &&
		(cd "$REMOTE_REPO" && echo "$HEAD" >expect && git rev-parse --verify HEAD >actual && test_cmp expect actual) &&
		grep "\[rejected\]" output && test_grep "Updates were rejected because" output
	'
	test_expect_${EXPECT_CAS_RESULT} 'force with lease aka cas' '
		HEAD=$(cd "$REMOTE_REPO" && git rev-parse --verify HEAD) &&
		test_when_finished '\''(cd "$REMOTE_REPO" && git update-ref HEAD "$HEAD")'\'' &&
		(cd "$LOCAL_REPO" && git push -v --force-with-lease=$BRANCH:$HEAD origin) &&
		git rev-parse --verify "$BRANCH" >expect &&
		(cd "$REMOTE_REPO" && git rev-parse --verify HEAD) >actual &&
		test_cmp expect actual
	'
}

# Helper: setup post-update hook that runs git update-server-info
setup_post_update_server_info_hook () {
	test_hook --setup -C "$1" post-update <<-\EOF &&
	exec git update-server-info
	EOF
	git -C "$1" update-server-info
}

# Askpass helpers (matching upstream's interface)
setup_askpass_helper() {
	test_expect_success 'setup askpass helper' '
		write_script "$TRASH_DIRECTORY/askpass" <<-\EOF &&
		echo >>"$TRASH_DIRECTORY/askpass-query" "askpass: $*" &&
		case "$*" in
		*Username*)
			what=user
			;;
		*Password*)
			what=pass
			;;
		esac &&
		cat "$TRASH_DIRECTORY/askpass-$what"
		EOF
		GIT_ASKPASS="$TRASH_DIRECTORY/askpass" &&
		export GIT_ASKPASS &&
		export TRASH_DIRECTORY
	'
}

set_askpass () {
	>"$TRASH_DIRECTORY/askpass-query" &&
	echo "$1" >"$TRASH_DIRECTORY/askpass-user" &&
	echo "$2" >"$TRASH_DIRECTORY/askpass-pass"
}

set_netrc () {
	# $HOME=$TRASH_DIRECTORY
	echo "machine $1 login $2 password $3" >"$TRASH_DIRECTORY/.netrc"
}

clear_netrc () {
	rm -f "$TRASH_DIRECTORY/.netrc"
}

# CGIPassAuth is an Apache feature not supported by test-httpd
enable_cgipassauth () {
	# Our test-httpd doesn't support CGIPassAuth
	# Set prereq so tests can check
	:
}

expect_askpass () {
	dest=$HTTPD_DEST${3+/$3}

	{
		case "$1" in
		none)
			;;
		pass)
			echo "askpass: Password for '$HTTPD_PROTO://$2@$dest': "
			;;
		both)
			echo "askpass: Username for '$HTTPD_PROTO://$dest': "
			echo "askpass: Password for '$HTTPD_PROTO://$2@$dest': "
			;;
		*)
			false
			;;
		esac
	} >"$TRASH_DIRECTORY/askpass-expect" &&
	test_cmp "$TRASH_DIRECTORY/askpass-expect" \
		 "$TRASH_DIRECTORY/askpass-query"
}
