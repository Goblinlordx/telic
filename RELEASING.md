# Releasing telic

The release process is CI-driven. A GitHub Release triggers
`cargo publish` to crates.io via `.github/workflows/release.yml`.

## One-time setup

### 1. crates.io account + API token

Visit https://crates.io, log in with GitHub, then:

1. Go to https://crates.io/settings/tokens
2. Click **New Token**
3. Name it `telic-release`; set scope to `publish-new` and `publish-update`
4. Copy the token (you won't see it again)

### 2. Add the token as a GitHub secret

In `https://github.com/Goblinlordx/telic/settings/secrets/actions`:

1. Click **New repository secret**
2. Name: `CARGO_REGISTRY_TOKEN`
3. Value: paste the crates.io token
4. **Save**

The release workflow reads `${{ secrets.CARGO_REGISTRY_TOKEN }}`.

### 3. First-time crate claim

Before any release workflow run, the `telic` name must be reserved on
crates.io. Do this once manually from a local checkout:

```sh
cargo login <YOUR_CRATES_IO_TOKEN>
cargo publish -p telic       # publishes v0.1.0
```

After the first publish, the name belongs to your account and all
subsequent releases go through CI.

## Per-release workflow

1. **Bump the version** in `crates/core/Cargo.toml`:
   ```toml
   [package]
   version = "0.2.0"
   ```

2. **Commit and push** to `main`:
   ```sh
   git commit -am "release: v0.2.0"
   git push
   ```
   CI runs `cargo test --workspace` on the push.

3. **Create a GitHub Release** once CI is green:
   ```sh
   gh release create v0.2.0 \
       --title "v0.2.0" \
       --notes "..." \
       --target main
   ```
   Or use the GitHub web UI at `.../releases/new`.

4. The **release workflow** fires on the published release:
   - Verifies the tag (`v0.2.0`) matches `crates/core/Cargo.toml` version
   - Runs the full test suite
   - Publishes `telic` to crates.io

Watch the run at
`https://github.com/Goblinlordx/telic/actions`.

## Version scheme

Semver. Until telic reaches 1.0:
- **0.x.y** → API may change on any minor bump
- breaking changes: bump the minor (`0.1.0` → `0.2.0`)
- additions / fixes: bump the patch (`0.1.0` → `0.1.1`)

After 1.0:
- **x.y.z** → breaking only on major bumps
