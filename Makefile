# Horizon IDE — common product targets (Linux / WSL / macOS bash).
# Windows: use ide\scripts\windows\*.ps1 (see ide/WINDOWS.md).
#
# One path: bootstrap → build → run = full product with Horizon built in.

.PHONY: ide-bootstrap ide-build ide-run ide-dev ide-sync ide-sidecar help

help:
	@echo "Horizon IDE targets:"
	@echo "  make ide-bootstrap   # clone Code-OSS, brand, sync contrib"
	@echo "  make ide-build       # sync contrib + npm ci + gulp compile-client"
	@echo "  make ide-run         # overlay + freshness compile + sidecar + launch"
	@echo "  make ide-dev         # fast: sync + compile-client + run"
	@echo "  make ide-sync        # sync ide/contrib/horizon only"
	@echo "  make ide-sidecar     # run horizon-server (writes ide/.cache/sidecar.url)"

ide-bootstrap:
	./ide/scripts/bootstrap.sh

ide-build:
	./ide/scripts/build.sh

ide-run:
	./ide/scripts/run.sh

ide-dev:
	./ide/scripts/dev.sh

ide-sync:
	./ide/scripts/sync-contrib.sh

ide-sidecar:
	./ide/scripts/run-sidecar.sh
