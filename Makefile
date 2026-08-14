PACKAGE_TARGET_DIR ?= target/publish

build:
	@cargo build

test:
	@cargo nextest run --all-features

characterize:
	@cargo run -p svg2excal-core --example characterize

test-compat:
	@test -d vendors/excalidraw/node_modules || corepack yarn --cwd vendors/excalidraw install --frozen-lockfile --ignore-scripts
	@cargo run -p svg2excal-core --example emit_compat
	@vendors/excalidraw/node_modules/.bin/vitest --config compat/vitest.config.mts run compat/excalidraw-compat.test.ts

test-fixtures:
	@cargo test -p svg2excal-core --test fixtures

test-visual: characterize
	@cd fixtures/baselines && shasum -a 256 -c SHA256SUMS
	@cargo run -p svg2excal-core --example emit_visual
	@test -d vendors/excalidraw/node_modules || corepack yarn --cwd vendors/excalidraw install --frozen-lockfile --ignore-scripts
	@vendors/excalidraw/node_modules/.bin/vitest --config compat/vitest.config.mts run compat/excalidraw-visual.test.ts
	@cargo run -p svg2excal-core --example verify_visual

test-hostile:
	@cargo test -p svg2excal-core --test hostile

fuzz:
	@for target in svg_preflight style_parser source_correlation geometry target_validator; do \
		cargo +nightly fuzz run $$target fuzz/corpus/$$target fixtures -- \
			-max_total_time=$${FUZZ_SECONDS:-30} -timeout=10 || exit 1; \
	done

fuzz-build:
	@cargo +nightly fuzz build

bench:
	@cargo bench -p svg2excal-core --bench conversion

bench-build:
	@cargo bench -p svg2excal-core --bench conversion --no-run

package:
	@cmp -s fixtures/rfc.svg crates/core/fixtures/rfc.svg || { \
		echo "The packaged RFC fixture is out of sync with fixtures/rfc.svg"; \
		exit 1; \
	}
	@for package_license in crates/core/LICENSE.md apps/cli/LICENSE.md apps/server/LICENSE.md; do \
		cmp -s LICENSE.md "$$package_license" || { \
			echo "$$package_license is out of sync with LICENSE.md"; \
			exit 1; \
		}; \
	done
	@cargo package --workspace --locked --allow-dirty \
		--target-dir "$(PACKAGE_TARGET_DIR)" $(CARGO_PACKAGE_FLAGS)
	@package_dir="$(PACKAGE_TARGET_DIR)/package"; \
	for crate_name in svg2excal-core svg2excal svg2excal-server; do \
		version=$$(cargo pkgid -p "$$crate_name" | sed 's/.*@//'); \
		archive="$$package_dir/$$crate_name-$$version.crate"; \
		test -f "$$archive" || { echo "Missing package archive: $$archive"; exit 1; }; \
		archive_size=$$(wc -c < "$$archive"); \
		test "$$archive_size" -le 10485760 || { \
			echo "Package exceeds crates.io's 10 MiB limit: $$archive"; \
			exit 1; \
		}; \
	done

update-font-assets:
	@cp vendors/excalidraw/scripts/woff2/assets/LiberationSans-Regular.ttf \
		crates/core/assets/fonts/LiberationSans-Regular.ttf
	@cp vendors/excalidraw/scripts/woff2/assets/NotoEmoji-Regular.ttf \
		crates/core/assets/fonts/NotoEmoji-Regular.ttf
	@uvx --from 'fonttools[woff]==4.60.2' pyftsubset \
		vendors/excalidraw/scripts/woff2/assets/Xiaolai-Regular.ttf \
		--output-file=crates/core/assets/fonts/Xiaolai-Regular-Basic-CJK.ttf \
		--unicodes='U+3000-303F,U+4E00-9FFF,U+F900-FAFF,U+FF00-FFEF' \
		--layout-features='*' --no-hinting --name-IDs='*' --name-languages='*'
	@shasum -a 256 crates/core/assets/fonts/*.ttf

verify: build test test-fixtures test-hostile test-compat test-visual fuzz-build bench-build package
	@cargo +nightly fmt --all -- --check
	@cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::pedantic
	@cargo audit
	@cargo deny check

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
	@test -n "$(VERSION)" || { \
		echo "Set VERSION to release, a SemVer level, or an explicit version"; \
		exit 1; \
	}
	@$(MAKE) verify
	@cargo release "$(VERSION)" --workspace --execute

update-submodule:
	@git submodule update --init --recursive --remote

.PHONY: build test characterize test-compat test-fixtures test-visual test-hostile fuzz fuzz-build bench bench-build package update-font-assets verify check-agent-sync release update-submodule
