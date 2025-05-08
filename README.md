# ShutterSort

[![Build ShutterSort](https://img.shields.io/github/actions/workflow/status/drangob/ShutterSort/build.yml?branch=main)](https://github.com/drangob/ShutterSort/actions/workflows/build.yml)
[![GitHub release (latest by date)](https://img.shields.io/github/v/release/drangob/ShutterSort)](https://github.com/drangob/ShutterSort/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-green.svg)](https://opensource.org/licenses/MIT)

ShutterSort is a command-line utility written in Rust to automatically organize your photo and video files into a structured directory hierarchy based on their metadata (EXIF data) or file system timestamps.

## Features

*   Organizes files by year, month, and day.
*   Optionally includes camera model in the folder structure (either as a prefix or suffix).
*   Can use EXIF data for date and camera model, or fall back to file creation/modification times.
*   Supports both one-time processing and continuous monitoring of a source directory.
*   Allows copying or moving files.
*   Option to keep original filenames or rename them to an ISO timestamp format.

## Installation

Build the project using Cargo:

```bash
cargo build --release
```

The executable will be located at `target/release/ShutterSort`.

### Downloading a Release

Alternatively, you can download pre-compiled binaries for your operating system from the [Latest Release page](https://github.com/drangob/ShutterSort/releases/latest).

**macOS Users:**

If you download the macOS binary, you might encounter a security warning because the application is not signed with an Apple Developer ID. As I am not paying Apple for the privilege of a developer account, you can self-sign the application locally to bypass this. After downloading, open your terminal, navigate to the directory containing the `ShutterSort-macos-arm64` (or similar) executable, and run the following command:

```bash
codesign --force --deep --sign - ./ShutterSort-macos-arm64
```

(Replace `ShutterSort-macos-arm64` with the actual name of the downloaded executable if it differs).
After self-signing, you might still need to grant an exception in "System Settings" > "Privacy & Security" the first time you run it.

## Usage

```bash
ShutterSort [OPTIONS] <COMMAND>
```

### Commands

The application supports two main commands:

*   `once`: Process all files in the source directory once and then exit.
*   `monitor`: Process existing files and then monitor the source directory for new files, processing them as they are added or modified.

### Options

These options are available for both `once` and `monitor` commands:

*   `-s, --source <SOURCE>`: (Required) Specifies the source directory containing the media files to process.
*   `-d, --destination <DESTINATION>`: (Required) Specifies the root destination directory where the organized files will be saved.
*   `-u, --use-modified`: If set, the application will use the file's last modified time if EXIF data extraction fails. By default, it uses the file's creation time as a fallback.
*   `--no-camera-model`: Disables the use of camera model information for organizing files. If this flag is not set, the camera model (extracted from EXIF or manually specified) will be used to create an additional subfolder.
*   `--camera-model-prefix`: If camera model organization is enabled, this flag makes the camera model part of the path prefix (e.g., `Destination/CameraModel/YYYY/MM/DD`). By default, the camera model is a suffix (e.g., `Destination/YYYY/MM/DD/CameraModel`).
*   `--manual-camera-model <MANUAL_CAMERA_MODEL>`: Allows you to manually specify a camera model name to be used for all files. This overrides any camera model extracted from EXIF data.
*   `--copy`: Copies files from the source to the destination directory instead of moving them. The default behavior is to move files.
*   `--keep-names`: Keeps the original filenames. By default, files are renamed to an ISO 8601 timestamp format (e.g., `YYYY-MM-DDTHH-MM-SS.ext`).

### Global Options

*   `-v, --verbose`: Enables verbose logging output (debug level). This can be helpful for troubleshooting.

## Examples

**Process files once, moving them and using camera model as a suffix:**

```bash
ShutterSort once -s /path/to/your/photos -d /path/to/organized/photos
```

**Monitor a directory, copy files, and use camera model as a prefix, falling back to modified time:**

```bash
ShutterSort monitor -s /mnt/camera_uploads -d /srv/media_library --copy --camera-model-prefix --use-modified
```

**Process files once, keeping original names and manually specifying the camera model:**

```bash
ShutterSort once -s ./raw_images -d ./sorted_collection --keep-names --manual-camera-model "MyPhone"
```

## License

This project is licensed under the MIT License. 