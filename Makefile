NAME    := fastdev
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml)
TARBALL        := ../$(NAME)-$(VERSION).tar.gz
TARBALL_VENDOR := ../$(NAME)-$(VERSION)-vendored.tar.gz

.DEFAULT_GOAL := help

.PHONY: help build release release-vendor

help:
	@echo "fastdev"
	@echo ""
	@echo "  make build           cargo build --release --workspace"
	@echo "  make release         build $(TARBALL) - plain source tarball,"
	@echo "                       no vendored deps"
	@echo "  make release-vendor  build $(TARBALL_VENDOR) - vendors Cargo deps"
	@echo "                       into vendor/ and ships them in the tarball,"
	@echo "                       needed so rpm/fastdev.spec's %build can run"
	@echo "                       cargo --offline (mock/COPR build roots have"
	@echo "                       no network access); this is what rpm/fastdev.spec's"
	@echo "                       Source0 expects"

build:
	cargo build --release --workspace

# Both tarballs are written outside the tree being archived on purpose:
# writing one inside it - even with a matching --exclude - still changes
# that directory's own entry list while tar is walking it, which tar
# reports as "file changed as we read it".

release:
	tar czf $(TARBALL) \
		--exclude=.git --exclude=build --exclude=target \
		--exclude=vendor --exclude=.cargo \
		-C .. "$(notdir $(CURDIR))"
	@echo "Wrote $(TARBALL)"

release-vendor:
	mkdir -p .cargo
	cargo vendor vendor > .cargo/config.toml
	tar czf $(TARBALL_VENDOR) \
		--exclude=.git --exclude=build --exclude=target \
		-C .. "$(notdir $(CURDIR))"
	@echo "Wrote $(TARBALL_VENDOR)"
