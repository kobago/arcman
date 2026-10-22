ADDR ?= 0.0.0.0:3000
BIN := $(or $(CARGO_INSTALL_ROOT),$(CARGO_HOME),$(HOME)/.cargo)/bin/arcman
UNIT := $(HOME)/.config/systemd/user/arcman.service

.PHONY: help frontend install service restart status logs uninstall test

help:
	@echo 'make install           build the frontend and install arcman to $(BIN)'
	@echo 'make service [ADDR=]   register and start the systemd user service (default ADDR=$(ADDR))'
	@echo 'make restart           restart the service after make install'
	@echo 'make status | logs     show the service state or follow its log'
	@echo 'make uninstall         stop and remove the service (data in ~/.arcman is kept)'
	@echo 'make test              run backend, frontend and end-to-end tests'

frontend:
	cd frontend && bun install --frozen-lockfile && bun run build

install: frontend
	cargo install --locked --path backend

service:
	@test -x $(BIN) || { echo '$(BIN) not found; run make install first' >&2; exit 1; }
	mkdir -p $(dir $(UNIT))
	sed -e 's|@BIN@|$(BIN)|' -e 's|@ADDR@|$(ADDR)|' deploy/arcman.service > $(UNIT)
	systemctl --user daemon-reload
	systemctl --user enable arcman
	systemctl --user restart arcman
	@loginctl show-user $$USER --property=Linger 2>/dev/null | grep -q yes || \
		echo 'note: run "sudo loginctl enable-linger $$USER" so arcman starts at boot without a login'

restart:
	systemctl --user restart arcman

status:
	systemctl --user status arcman

logs:
	journalctl --user -u arcman -f

uninstall:
	-systemctl --user disable --now arcman
	rm -f $(UNIT)
	systemctl --user daemon-reload

test:
	cd backend && cargo test
	cd frontend && bun run test && bun run e2e
