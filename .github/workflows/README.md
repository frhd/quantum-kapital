# GitHub Actions Workflows

This directory contains GitHub Actions workflows for building and releasing Quantum Kapital.

## Workflows

### 1. CI (`ci.yml`)
Runs on every push to main and on pull requests. It:
- Checks Rust code formatting
- Runs clippy linting
- Runs Rust tests on all platforms
- Builds and type-checks the frontend
- Performs a test build on all platforms

### 2. Build (`build.yml`)
Can be triggered manually or on push to main/tags. It:
- Builds the app for all platforms (Windows, macOS Intel, macOS Apple Silicon, Linux)
- Creates draft releases with artifacts
- Uploads build artifacts for 30 days

### 3. Release (`release.yml`)
Triggered when pushing version tags (e.g., `v1.0.0`). It:
- Creates a GitHub release
- Builds signed binaries for all platforms
- Automatically publishes the release

## Required Secrets

For basic builds (no code signing):
- `GITHUB_TOKEN` - Automatically provided by GitHub Actions

For Tauri updater signing (optional):
- `TAURI_SIGNING_PRIVATE_KEY` - Private key for update signatures
- `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` - Password for the private key

For macOS code signing (optional):
- `APPLE_CERTIFICATE` - Base64 encoded .p12 certificate
- `APPLE_CERTIFICATE_PASSWORD` - Certificate password
- `APPLE_SIGNING_IDENTITY` - Certificate identity (e.g., "Developer ID Application: Your Name")
- `APPLE_ID` - Apple ID for notarization
- `APPLE_PASSWORD` - App-specific password for notarization
- `APPLE_TEAM_ID` - Apple Developer Team ID

For Windows code signing (optional):
- `WINDOWS_CERTIFICATE` - Base64 encoded .pfx certificate
- `WINDOWS_CERTIFICATE_PASSWORD` - Certificate password

## Usage

### Manual Build
1. Go to Actions tab in your GitHub repository
2. Select "Build Quantum Kapital"
3. Click "Run workflow"
4. Select branch and click "Run workflow"

### Creating a Release
1. Update version in `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`
2. Commit changes
3. Create and push a tag:
   ```bash
   git tag v1.0.0
   git push origin v1.0.0
   ```
4. The release workflow will automatically build and publish

### Setting up Code Signing

#### macOS
1. Export your Developer ID certificate as .p12
2. Convert to base64: `base64 -i certificate.p12 | pbcopy`
3. Add as `APPLE_CERTIFICATE` secret
4. Add other Apple-related secrets

#### Windows
1. Export your code signing certificate as .pfx
2. Convert to base64: `certutil -encode certificate.pfx certificate.txt`
3. Copy contents (without headers) and add as `WINDOWS_CERTIFICATE` secret
4. Add certificate password as `WINDOWS_CERTIFICATE_PASSWORD`

## Platform-Specific Builds

The workflows build for:
- **Windows**: 64-bit Windows 10/11 (`.msi` and `.exe`)
- **macOS**: Apple Silicon (M1+) and Intel (`.dmg` and `.app`)
- **Linux**: Ubuntu/Debian (`.deb`), Fedora/RHEL (`.rpm`), and universal (`.AppImage`)

## Troubleshooting

### Build Failures
- Check the Actions tab for detailed error logs
- Ensure all dependencies are properly specified in `package.json` and `Cargo.toml`
- Verify that the Rust code passes `cargo check` locally

### Missing Artifacts
- Artifacts are retained for 30 days
- Check the workflow run summary for artifact links
- Ensure the build completed successfully

### Code Signing Issues
- Verify certificates haven't expired
- Check that secrets are properly set (no extra spaces or newlines)
- For macOS, ensure you have the correct provisioning profiles
