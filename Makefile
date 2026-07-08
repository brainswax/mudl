# Makefile for MUDL project

.PHONY: fmt clippy check test test-m5 test-m6 build run-repl run-irc run-slack clean help

# Default target
all: check test build

help: ## Show this help message
	@echo "MUDL Development Makefile"
	@echo "Available targets:"
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "  %-20s %s\n", $$1, $$2}'

fmt: ## Format code with rustfmt
	cargo fmt --all

clippy: ## Run clippy linter
	cargo clippy --all-targets --all-features -- -D warnings || echo "clippy not installed; run rustup component add clippy"

check: ## Run cargo check
	cargo check --all-targets --all-features

test: ## Run tests
	cargo test --all-targets --all-features

test-m5: ## Run M5 multi-user and IRC tests
	cargo test gateway:: && cargo test irc::

test-m6: ## Run M6 Slack transport tests
	cargo test slack::

build: ## Build the project
	cargo build --all-targets --all-features

run-repl: ## Run the REPL
	cargo run --bin repl

run-irc: ## Run the IRC bot (set IRC_MOCK=1 for stdin mock mode)
	cargo run --bin irc

run-slack: ## Run the Slack bot (set SLACK_MOCK=1 for stdin mock mode)
	cargo run --bin slack

clean: ## Clean build artifacts
	cargo clean

# Additional common tasks
install: ## Install dependencies (if needed)
	cargo build

dev: fmt check clippy test ## Run development checks

# For SQLite, ensure database setup if needed
db-setup: ## Placeholder for DB setup
	@echo "Ensure DATABASE_URL is set and run migrations if applicable"