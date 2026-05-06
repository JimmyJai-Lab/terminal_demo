.PHONY: help dev build-wasm build-wasm-dev build-web build clean install native

help: ## Show help information
	@echo "terminal_demo - Available commands:"
	@echo ""
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-15s\033[0m %s\n", $$1, $$2}'

install: ## Install all dependencies
	@echo "Checking Rust WASM target..."
	@rustup target add wasm32-unknown-unknown || true
	@echo "Checking wasm-bindgen-cli..."
	@cargo install wasm-bindgen-cli || true
	@echo "Installing frontend dependencies..."
	@cd www && bun install

native: ## Run native binary (fast iteration)
	@cargo run -p terminal_demo --bin terminal_demo

build-wasm: ## Build WASM (release mode)
	@./scripts/build-wasm.sh --release

build-wasm-dev: ## Build WASM (debug mode)
	@./scripts/build-wasm.sh

build-web: ## Build frontend
	@cd www && bun run build

build: build-wasm build-web ## Build complete project (WASM + frontend)

dev: build-wasm-dev ## Start development server (WASM + Vite at localhost:3000)
	@cd www && bun install && bun run dev

clean: ## Clean build artifacts
	@echo "Cleaning build artifacts..."
	@rm -rf www/dist
	@rm -rf www/src/wasm/*.js www/src/wasm/*.wasm
	@cargo clean
