# Releasing OSULZER ImageOptimizer

Releases are built, signed and published by GitHub Actions (`.github/workflows/release.yml`) when a tag
`vX.Y.Z` is pushed. The program checks `https://api.github.com/repos/OSULZER/osulzer-imageoptimizer/releases/latest`
and only installs an update whose signature matches the public key built into the exe.

## One-time setup

### 1. Create the signing key

```
cargo run --example keygen
```

- Writes the public key to `assets/update-public-key.txt` – commit this file.
- Prints the **private key once**. Store it in your password manager and add it as a GitHub secret:
  repository **Settings → Secrets and variables → Actions → New repository secret**, name
  `UPDATE_SIGNING_KEY`.
- If the private key is lost, installed copies can never receive updates again. If it leaks, anyone could
  sign updates: create a new key, publish a release with the new public key, and tell users to download it
  manually once.
- The tool refuses to overwrite an existing real key.

### 2. Repositories

1. On Forgejo create the repository `osulzer-imageoptimizer` (empty).
2. On GitHub create `OSULZER/osulzer-imageoptimizer` (public, empty, no README).
3. In Forgejo: repository **Settings → Repository → Mirror settings → Push mirror**:
   - URL `https://github.com/OSULZER/osulzer-imageoptimizer.git`
   - user name: your GitHub user, password: a GitHub token (fine-grained, repository
     `osulzer-imageoptimizer`, permissions *Contents: read and write* **and** *Workflows: read and write*;
     classic tokens need the scopes `repo` and `workflow`). Without the workflow permission GitHub refuses
     the push because it contains `.github/workflows/`.
   - Enable *sync when commits are pushed*. Forgejo mirrors branches **and tags**.

### GitHub repository settings

- **About → Description:** Make camera photos web-ready with a right-click: resize, compress, rename for SEO
  and embed AI-written alt texts. Free for Windows, macOS and Linux, with command line and MCP server for AI
  agents. By OSULZER – osulzer.at
- **About → Website:** `https://www.osulzer.at`
- **About → Topics:** `image-optimization` `image-compression` `webp` `jpeg` `png` `seo` `alt-text` `wordpress`
  `windows` `macos` `linux` `context-menu` `rust` `egui` `mcp` `mcp-server` `ai` `ollama` `claude`
- **Settings → General → Social preview:** upload `docs/images/social-preview.jpg` (1280×640).

### 3. Publish the public code

The internal repository keeps its full history. The public one starts fresh, without internal planning
documents:

```
powershell -File scripts/export-public.ps1 -Target ..\osulzer-imageoptimizer-public
cd ..\osulzer-imageoptimizer-public
git remote add origin http://<forgejo>/<user>/osulzer-imageoptimizer.git
git push -u origin main
```

The script stops if it finds LAN addresses, user paths or e-mail addresses.

## Every release

1. Raise `version` in `Cargo.toml` (e.g. `1.0.1`), run `cargo build` so `Cargo.lock` follows, commit.
   The signature covers tag and exe, so the tag must be exactly `v` + this version.
2. Export and push as above (or work directly in the public repository).
3. Tag and push:
   ```
   git tag v1.0.1
   git push origin v1.0.1
   ```
4. The workflow checks that the tag matches `Cargo.toml` and that a real key and the secret exist, then runs
   the tests and builds on Windows, Linux (x86_64 and arm64) and macOS (universal app), signs the file the
   updater downloads on each system and publishes the release with:
   - `osulzer-imageoptimizer.exe` (+ `.sig`)
   - `osulzer-imageoptimizer-linux-x86_64`, `-linux-arm64` (+ `.sig`, and `.tar.gz` for the first download)
   - `osulzer-imageoptimizer-macos` (+ `.sig`, the program inside the app) and `osulzer-imageoptimizer-macos.zip`
   - `THIRD-PARTY-LICENSES.html`
5. Installed copies see the update within a day (or via **Settings → Check now**).

## SmartScreen

The exe is not code-signed (Authenticode). Windows shows "Windows protected your PC" on first start; users
click **More info → Run anyway**. A certificate (e.g. Microsoft Artifact Signing, about 10 USD per month) can
be added later as an extra step in the workflow.

---

## Kurzfassung auf Deutsch

1. **Schlüssel:** `cargo run --example keygen` – öffentlichen Schlüssel (`assets/update-public-key.txt`)
   committen; den einmal angezeigten privaten Schlüssel im Passwortmanager sichern und bei GitHub als
   Secret `UPDATE_SIGNING_KEY` eintragen. Verloren = keine Updates mehr für bestehende Installationen.
2. **Repos:** auf Forgejo `osulzer-imageoptimizer`, auf GitHub `OSULZER/osulzer-imageoptimizer` anlegen;
   in Forgejo einen Push-Mirror nach GitHub mit einem GitHub-Token einrichten.
3. **Öffentlicher Stand:** `scripts/export-public.ps1 -Target <ordner>` erzeugt ein Repo mit frischer
   Historie ohne interne Dokumente; dort `origin` setzen und pushen.
4. **Release:** Version in `Cargo.toml` erhöhen, Tag `vX.Y.Z` pushen – der Rest läuft automatisch.
