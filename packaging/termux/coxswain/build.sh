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
}

termux_step_make() {
	cargo build --jobs "$TERMUX_PKG_MAKE_PROCESSES" --target "$CARGO_TARGET_NAME" --release -p coxswain
}

termux_step_make_install() {
	install -Dm700 -t "$TERMUX_PREFIX/bin" "target/${CARGO_TARGET_NAME}/release/coxswain"
	ln -sf coxswain "$TERMUX_PREFIX/bin/cox"
	install -Dm600 -t "$TERMUX_PREFIX/share/man/man1" crates/coxswain/coxswain.1
}
