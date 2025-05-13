.PHONY: default
default:
	./gen.sh

.PHONY: test
test:
	go test ./...
