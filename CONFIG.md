# Configuration Guide

Tabulite uses a TOML-based configuration file to customize application settings.

## Configuration File Location

### XDG_CONFIG_HOME Environment Variable

On all platforms (including macOS), if the `$XDG_CONFIG_HOME` environment variable is set, Tabulite will use that directory instead of the platform default:

```bash
# Example: Use a custom config directory
export XDG_CONFIG_HOME="$HOME/.config"
# Tabulite will now look for: $HOME/.config/tabulite/config.toml
```

If `$XDG_CONFIG_HOME` is not set, Tabulite looks for the configuration file in the XDG config directory:

- **Linux**: `~/.config/tabulite/config.toml`
- **macOS**: `~/Library/Application Support/tabulite/config.toml`
- **Windows**: `%APPDATA%\tabulite\config.toml`

This follows the XDG Base Directory specification and allows for consistent configuration management across different platforms.

### Custom Configuration Path
You can override the default configuration file location by providing a custom path with the -c --config command line argument.

## Configuration File Format

The configuration file uses TOML format. Here's an example:

```toml
# Theme to use for the application
theme = "Default Dark"
```

## Configuration Options

### `theme`

- **Type**: String
- **Default**: `"Default Light"`
- **Description**: Specifies the theme to use for the application interface.

**Examples**:
```toml
theme = "Default Dark"
theme = "Everforest Dark"
theme = "Everforest Light"
```
