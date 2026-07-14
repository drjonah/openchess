# NNUE training data tools (Q2-01)

See **[research/nnue-training.md](../../research/nnue-training.md)** for the full pipeline.

```bash
# End-to-end fixture (developer machine)
./tools/nnue-data/run_fixture.sh

# Or via CLI
cargo run -- nnue-data fixture
cargo run -- nnue-data generate --help
cargo run -- nnue-data validate tools/nnue-data/out/fixture_bullet.txt
```

Generated files land in `out/` (gitignored).
