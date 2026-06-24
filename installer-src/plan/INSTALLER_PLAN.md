# Forge Installer Plan

## Goal
Build a minimal, professional, GUI-based installer for Forge that supports:
- fresh install
- upgrade
- uninstall

The installer should support both:
- prebuilt GitHub release binaries
- build-from-source installation

The installer must work across a broad range of Linux distributions, use the least practical number of dependencies, and keep the user experience simple.

## Current Scope

The installer will focus on:
- installing Forge binaries correctly
- managing upgrades and uninstall cleanly
- handling dependency installation automatically
- creating desktop integration
- providing clear status, logs, and failure recovery

Out of scope for this installer version:
- bundled config templates
- theme assets
- telemetry
- CLI mode
- offline installation
- repair mode
- advanced details panels

## Core Behavior

### Install state
- On startup, detect Forge by checking:
  - binary location
  - version file
- If Forge is not installed:
  - show install only
- If Forge is installed:
  - show upgrade and uninstall only

### Install modes
- Support a fresh install from GitHub release binaries
- Support a fresh install by building Forge from source
- Support upgrades with the same two choices

### Package management
- Automatically install missing build-time and runtime dependencies
- Use the dependency lists in:
  - `build_dependencies.txt`
  - `runtime_dependencies.txt`
- Support multiple package managers automatically through distro detection
- If a distro or package manager is unsupported:
  - stop with a clear error
  - tell the user to install packages manually
- During source builds:
  - remove only the build-time packages installed by the installer
  - never remove packages that were already present

### Privileges
- Prompt for the root password at the start
- Reuse that authentication for privileged steps throughout the session

### Install locations
- Binary path:
  - `/opt/Forge/forge`
- Symlink targets:
  - global install: `/usr/local/bin/forge`
  - current-user install: `~/.local/bin/forge`
- Launch command should be:
  - `forge`

### User scope
- Let the user choose:
  - global install
  - current-user install
- Use the same session reminder message for both scopes after install

## GUI Plan

### Layout
- Step-by-step wizard
- Minimal number of pages
- Show the selected install mode on every page
- Title:
  - `Forge Installer`

### Visual style
- Light Forge branding
- Minimal, professional look
- English only

### Screens

#### 1. Start screen
- Show current install status
- Show available action on the same screen
- If Forge is already installed:
  - show upgrade and uninstall
- If Forge is not installed:
  - show install only

#### 2. Mode selection
- Choose install scope:
  - global
  - current-user
- Choose installation source:
  - GitHub release binary
  - build from source

#### 3. Prerequisite and dependency step
- Combine prerequisite checks and dependency installation into one step
- Check runtime prerequisites first
- Install missing runtime and build dependencies automatically
- Show package names briefly in a compact status line
- Show a single simple progress bar

#### 4. Install or upgrade execution
- For source builds:
  - build after dependencies are installed
  - clean build artifacts after success
- For binary installs:
  - download fresh from the latest GitHub release
- Do not cache release assets locally
- Do not show ETA
- Do not show release notes

#### 5. Success screen
- Install success:
  - show a short success message
  - offer a launch button
  - finish
- Upgrade success:
  - show success
  - offer a relaunch button
  - finish
- Uninstall success:
  - show success
  - confirm cleanup is done
  - offer reinstall

#### 6. Help/About screen
- Include:
  - Forge GitHub repository URL
  - license text
  - version info

## Logging and Diagnostics

- Show both:
  - short summary
  - detailed logs
- Do not expose a log viewer inside the UI
- Show the log file path only on:
  - error screens
  - final screens
- Write logs to a temporary file on disk
- Keep logs until reboot, then let the OS clean them up

## Failure Handling

- If an install fails:
  - clean up partial changes
  - prompt the user to retry or quit
- If an upgrade fails:
  - restore from temporary backups
  - remove backups on success
- Allow retrying the last failed step
- Do not support headless fallback
- Fail clearly if the GUI cannot start

## Upgrade Behavior

- Upgrades should:
  - detect current install state
  - back up overwritten files temporarily
  - restore on failure
  - clean backups on success
  - refresh desktop/menu cache if supported by the platform
- If available easily, show:
  - current version
  - target version
- Use Git tags for version reporting
- Do not require a restart after upgrade
- Show the normal session/PATH reminder only

## Uninstall Behavior

- Uninstall should:
  - be available only inside the installer
  - be forward-only once started
  - remove everything by default
  - offer a checkbox to preserve config
- Remove:
  - binary
  - symlink
  - desktop entry
  - icon
  - installed state metadata
- If config removal is checked:
  - remove config too
- Provide a separate uninstall component inside the installer implementation

## Desktop Integration

- Create:
  - desktop entry
  - application icon
  - launcher integration
  - menu category entry
- Refresh desktop/menu cache when supported
- Update existing desktop entry or icon during upgrade
- Do not warn before overwriting desktop integration files

## State Tracking

- Keep a small local state file with:
  - installed version
  - install scope
  - cleanup metadata
- Store it under:
  - user config directory for current-user installs
  - system config directory for global installs

## Networking

- Require network access
- Always download fresh
- Use the system environment for proxy handling
- Do not add a proxy configuration UI

## Verification

- Verification policy is not finalized yet
- Do not display checksum information in the UI for now
- Do not enforce tag/release-name matching
- Do validate the downloaded filename against the expected Forge artifact name

## Build Phases

### Phase 1: Installer structure
- Create the installer project layout
- Add the GUI shell
- Add the startup detection logic
- Add the mode selection flow

### Phase 2: Dependency orchestration
- Parse build and runtime dependency lists
- Detect distro and package manager
- Implement auto-install behavior
- Implement cleanup of installer-added build dependencies

### Phase 3: Source and binary install paths
- Add GitHub release fetching
- Add source build path
- Add install destination and symlink handling
- Add upgrade path

### Phase 4: Desktop integration
- Create desktop entry
- Install icon
- Register launcher/menu category
- Refresh cache when possible

### Phase 5: Uninstall and rollback
- Add uninstall path
- Add temporary backup and restore behavior
- Add cleanup on failure

### Phase 6: UX polish
- Add summary and detailed logs
- Add compact package status line
- Add final screens
- Add help/about screen

## Acceptance Criteria

The installer is ready when:
- it can install Forge from a GitHub release binary
- it can build Forge from source
- it can upgrade an existing install
- it can uninstall cleanly
- it auto-installs dependencies
- it cleans up after failures
- it creates desktop integration
- it keeps logs and state in a predictable way
- it remains minimal and professional

## Notes

- The latest user clarification narrows the installer toward binary installation as the main thing it must do well.
- The plan deliberately avoids adding extra asset packaging until explicitly requested.
