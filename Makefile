build:
	@cargo build

test:
	@cargo nextest run --all-features

characterize:
	@cargo run -p svg2excal-core --example characterize

test-compat:
	@test -d vendors/excalidraw/node_modules || corepack yarn --cwd vendors/excalidraw install --frozen-lockfile --ignore-scripts
	@vendors/excalidraw/node_modules/.bin/vitest --config compat/vitest.config.mts run compat/excalidraw-compat.test.ts

test-fixtures:
	@cargo test -p svg2excal-core --test fixtures

test-visual: characterize
	@cd fixtures/baselines && shasum -a 256 -c SHA256SUMS

test-hostile:
	@cargo test -p svg2excal-core --test hostile

verify: build test test-fixtures test-hostile test-compat
	@cargo +nightly fmt --all -- --check
	@cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::pedantic

check-agent-sync:
	@cmp -s CLAUDE.md AGENTS.md || { \
		echo "AGENTS.md must stay in sync with CLAUDE.md"; \
		echo "Update both files with the same shared project instructions."; \
		exit 1; \
	}
	@tmp_dir=$$(mktemp -d); \
	trap 'rm -rf "$$tmp_dir"' EXIT; \
	cp -R .claude/skills "$$tmp_dir/expected-skills"; \
	find "$$tmp_dir/expected-skills" -name SKILL.md -exec perl -0pi -e 's/CLAUDE\.md/AGENTS.md/g; s/Claude/Codex/g; s/claude/codex/g' {} +; \
	diff -ru --exclude agents "$$tmp_dir/expected-skills" .agents/skills || { \
		echo "Codex skills must stay in sync with Claude skills after Claude-to-Codex renaming."; \
		echo "Update .claude/skills first, then mirror the shared content into .agents/skills."; \
		exit 1; \
	}

release:
	@cargo release tag --execute
	@git cliff -o CHANGELOG.md
	@git commit -a -n -m "Update CHANGELOG.md" || true
	@git push origin master
	@cargo release push --execute

update-submodule:
	@git submodule update --init --recursive --remote

.PHONY: build test characterize test-compat test-fixtures test-visual test-hostile verify check-agent-sync release update-submodule
