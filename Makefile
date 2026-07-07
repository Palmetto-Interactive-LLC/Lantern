SHELL := /bin/bash

.PHONY: help fmt lint test build docs-links verify audit shellcheck actionlint gitleaks security package-smoke install-smoke release-signing-smoke

help:
	@printf 'Lantern quality commands:\n'
	@printf '  make verify         Rust fmt, strict clippy, tests, release build, docs links\n'
	@printf '  make security       cargo audit, shellcheck, actionlint, gitleaks\n'
	@printf '  make package-smoke  Package the current host release tarball into a temp dir\n'
	@printf '  make install-smoke  Run source installer under an isolated HOME\n'
	@printf '  make release-signing-smoke  Verify local signed-tag release setup\n'

fmt:
	cargo fmt --check

lint:
	cargo clippy --all-targets -- -D warnings

test:
	cargo test

build:
	cargo build --release

docs-links:
	python3 scripts/check-doc-links.py

verify: fmt lint test build docs-links

audit:
	cargo audit

shellcheck:
	shellcheck scripts/install.sh scripts/lantern-up.sh scripts/lantern-down.sh scripts/lantern-doctor.sh scripts/setup-iterm.sh scripts/startwork.sh scripts/stopwork.sh scripts/package-release.sh scripts/check-release-signing.sh

actionlint:
	actionlint -color=false .github/workflows/*.yml

gitleaks:
	gitleaks detect --source . --redact --no-banner

security: audit shellcheck actionlint gitleaks

package-smoke: build
	tmp_dist="$$(mktemp -d)"; \
	trap 'rm -rf "$$tmp_dist"' EXIT; \
	host="$$(rustc -vV | awk '/^host:/ {print $$2}')"; \
	DIST_DIR="$$tmp_dist" scripts/package-release.sh v0.0.0-smoke "$$host"; \
	test -f "$$tmp_dist/SHA256SUMS"; \
	test -x "$$tmp_dist/install-lantern.sh"; \
	grep -F "install-lantern.sh" "$$tmp_dist/SHA256SUMS" >/dev/null; \
	tar -tzf "$$tmp_dist/lantern-v0.0.0-smoke-$$host.tar.gz" >/dev/null

install-smoke:
	tmp_home="$$(mktemp -d)"; \
	trap 'rm -rf "$$tmp_home"' EXIT; \
	rustup_home="$${RUSTUP_HOME:-$$HOME/.rustup}"; \
	cargo_home="$${CARGO_HOME:-$$HOME/.cargo}"; \
	HOME="$$tmp_home" RUSTUP_HOME="$$rustup_home" CARGO_HOME="$$cargo_home" ./scripts/install.sh; \
	"$$tmp_home/.lantern/bin/lantern" --version; \
	test -x "$$tmp_home/.lantern/bin/lantern-up"; \
	test -x "$$tmp_home/.lantern/bin/lantern-down"; \
	test -x "$$tmp_home/.lantern/bin/lantern-doctor"; \
	test -x "$$tmp_home/.lantern/bin/lantern-install"; \
	test -x "$$tmp_home/.lantern/bin/lantern-setup-iterm"; \
	test -x "$$tmp_home/.lantern/bin/startwork"; \
	test -x "$$tmp_home/.lantern/bin/stopwork"; \
	test -f "$$tmp_home/Library/LaunchAgents/com.lantern.relay.plist"

release-signing-smoke:
	scripts/check-release-signing.sh
