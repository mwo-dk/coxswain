TERMUX_PKG_HOMEPAGE=https://github.com/mwo-dk/coxswain
TERMUX_PKG_DESCRIPTION="Two-panel file manager for the terminal, Norton Commander style"
TERMUX_PKG_LICENSE="MIT"
TERMUX_PKG_MAINTAINER="@termux"
TERMUX_PKG_VERSION="2.1.0"
TERMUX_PKG_SRCURL="https://github.com/mwo-dk/coxswain/archive/refs/tags/v${TERMUX_PKG_VERSION}.tar.gz"
TERMUX_PKG_SHA256=a886582084e866ac50130898159a2a299317d58f419f9940ffe231b418fdce7f
TERMUX_PKG_RECOMMENDS="git, termux-api"
TERMUX_PKG_BUILD_IN_SRC=true
TERMUX_PKG_AUTO_UPDATE=true

termux_step_pre_configure() {
	termux_setup_rust

	# trash-rs leaves Android out; treat it as the freedesktop.org trash, as yazi does.
	: "${CARGO_HOME:=$HOME/.cargo}"
	export CARGO_HOME
	cargo fetch --locked --target "$CARGO_TARGET_NAME"
	rm -rf vendor/trash
	mkdir -p vendor
	cp -r "$CARGO_HOME"/registry/src/*/trash-5.*/ vendor/trash
	find vendor/trash -type f -print0 | xargs -0 sed -i \
		-e 's|"android"|"disabling_this_because_it_is_for_building_an_apk"|g' \
		-e "s|/tmp|$TERMUX_PREFIX/tmp|g"
	patch -p1 -d vendor/trash < "$TERMUX_PKG_BUILDER_DIR/trash-rs-implement-get_mount_points-android.diff"
	# Cargo.toml ends with its [patch.crates-io] table.
	echo 'trash = { path = "./vendor/trash" }' >> Cargo.toml
}

termux_step_make() {
	cargo build --jobs "$TERMUX_PKG_MAKE_PROCESSES" --target "$CARGO_TARGET_NAME" --release -p coxswain
}

termux_step_make_install() {
	install -Dm700 -t "$TERMUX_PREFIX/bin" "target/${CARGO_TARGET_NAME}/release/coxswain"
	ln -sf coxswain "$TERMUX_PREFIX/bin/cox"
	install -Dm600 -t "$TERMUX_PREFIX/share/man/man1" crates/coxswain/coxswain.1
	ln -sf coxswain.1 "$TERMUX_PREFIX/share/man/man1/cox.1"
}
