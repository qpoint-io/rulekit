.PHONY: default
default:
	./gen.sh

.PHONY: test
test:
	go test ./...

.PHONY: wasm
wasm:
	@echo "Building rulekit WASM..."
	@cd cmd/wasm && ./build.sh
