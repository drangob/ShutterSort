use anyhow::{Context, Result};
use chrono::{DateTime, Datelike, TimeZone, Utc};
use clap::{Parser, Subcommand};
use log::{error, info, warn, LevelFilter, debug};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::sync::mpsc::channel;
use walkdir::WalkDir;
use mime;

#[derive(clap::Args, Debug)]
struct SharedArgs {
    #[arg(short, long, help = "Source directory containing media files")]
    source: String,
    #[arg(short, long, help = "Destination directory for organized files")]
    destination: String,
    #[arg(short, long, default_value_t = false, help = "On EXIF failure, use file's last modified time (default: use creation time).")]
    use_modified: bool,
    #[arg(long = "no-camera-model", action = clap::ArgAction::SetTrue, help = "Disable camera model extraction for folder organization. If not set, camera model will be used.")]
    no_camera_model: bool,
    #[arg(long, default_value_t = false, help = "Use camera model as a prefix in the destination path (e.g., Camera/YYYY/MM/DD). Default is suffix (YYYY/MM/DD/Camera).")]
    camera_model_prefix: bool,
    #[arg(long, help = "Manually specify camera model")]
    manual_camera_model: Option<String>,
    #[arg(long, default_value_t = false, help = "Copy files instead of moving (default is move)")]
    copy: bool,
    #[arg(long, default_value_t = false, help = "Keep original filenames instead of renaming to ISO timestamp (default is rename)")]
    keep_names: bool,
}

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(short, long, action = clap::ArgAction::SetTrue, global = true, help = "Enable verbose logging (debug level)")]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    #[command(about = "Process files once without monitoring")]
    Once {
        #[clap(flatten)]
        shared: SharedArgs,
    },
    #[command(about = "Monitor source directory and automatically process new files")]
    Monitor {
        #[clap(flatten)]
        shared: SharedArgs,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let default_log_level = if cli.verbose {
        LevelFilter::Debug.as_str() 
    } else {
        LevelFilter::Info.as_str()
    };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default_log_level)).init();

    match &cli.command {
        Commands::Once { shared } => {
            process_directory(&shared.source, &shared.destination, shared.use_modified, !shared.no_camera_model, shared.camera_model_prefix, shared.manual_camera_model.as_ref(), shared.copy, shared.keep_names)?;
        }
        Commands::Monitor { shared } => {
            monitor_directory(&shared.source, &shared.destination, shared.use_modified, !shared.no_camera_model, shared.camera_model_prefix, shared.manual_camera_model.as_ref(), shared.copy, shared.keep_names)?;
        }
    }
    Ok(())
}

fn process_directory(source: &str, destination: &str, use_modified: bool, use_camera_model: bool, camera_model_is_prefix: bool, manual_camera_model: Option<&String>, copy_files: bool, keep_names: bool) -> Result<()> {
    info!("Processing directory: {}", source);
    let source_path = Path::new(source);
    for entry in WalkDir::new(source_path).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            let file_path = entry.path();
            match process_file(file_path, destination, use_modified, use_camera_model, camera_model_is_prefix, manual_camera_model, copy_files, keep_names) {
                Ok(_) => {},
                Err(e) => warn!("Failed to process file {}: {}", file_path.display(), e),
            }
        }
    }
    delete_empty_folders(source)?;
    info!("Directory processing complete");
    Ok(())
}

fn monitor_directory(source: &str, destination: &str, use_modified: bool, use_camera_model: bool, camera_model_is_prefix: bool, manual_camera_model: Option<&String>, copy_files: bool, keep_names: bool) -> Result<()> {
    info!("Starting to monitor directory: {}", source);
    // Initial processing of existing files
    process_directory(source, destination, use_modified, use_camera_model, camera_model_is_prefix, manual_camera_model, copy_files, keep_names)?;
    // Set up file watcher
    let (tx, rx) = channel();
    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;
    watcher.watch(Path::new(source).as_ref(), RecursiveMode::Recursive)?;
    info!("Watching for changes...");
    loop {
        match rx.recv() {
            Ok(Ok(event)) => handle_fs_event(event, source, destination, use_modified, use_camera_model, camera_model_is_prefix, manual_camera_model, copy_files, keep_names)?,
            Ok(Err(e)) => error!("Watch error: {:?}", e),
            Err(e) => {
                error!("Watch channel error: {:?}", e);
                break;
            }
        }
    }
    Ok(())
}

fn handle_fs_event(event: Event, source: &str, destination: &str, use_modified: bool, use_camera_model: bool, camera_model_is_prefix: bool, manual_camera_model: Option<&String>, copy_files: bool, keep_names: bool) -> Result<()> {
    if let notify::EventKind::Create(_) | notify::EventKind::Modify(_) = event.kind {
        for path in event.paths {
            if path.is_file() {
                match process_file(&path, destination, use_modified, use_camera_model, camera_model_is_prefix, manual_camera_model, copy_files, keep_names) {
                    Ok(_) => {},
                    Err(e) => warn!("Failed to process file {}: {}", path.display(), e),
                }
            }
        }
    }
    delete_empty_folders(source)?;
    Ok(())
}

fn process_file(file_path: &Path, destination: &str, use_modified: bool, use_camera_model: bool, camera_model_is_prefix: bool, manual_camera_model: Option<&String>, copy_files: bool, keep_names: bool) -> Result<()> {
    let mut dest_path_option: Option<PathBuf> = None;

    let is_media_file = if let Some(ext) = file_path.extension().and_then(OsStr::to_str) {
        let mime_type = mime_guess::from_ext(ext).first_or_octet_stream();
        mime_type.type_() == mime::IMAGE || mime_type.type_() == mime::VIDEO
    } else {
        false
    };

    if is_media_file {
        debug!("Processing media file: {}", file_path.display());
        let date_time = extract_date(file_path, use_modified)
            .context(format!("Failed to extract date from {}", file_path.display()))?;

        let camera_model_str = if let Some(manual_model) = manual_camera_model {
            manual_model.clone()
        } else if use_camera_model {
            extract_camera_model(file_path).unwrap_or_else(|_| "Unknown".to_string())
        } else {
            String::new()
        };
        dest_path_option = Some(create_destination_path(destination, &date_time, &camera_model_str, file_path, keep_names, camera_model_is_prefix)?);
    } else {
        debug!("File is not a media file (or has no/invalid extension): {}", file_path.display());
        if !copy_files {
            // Only move non-media files if in move mode
            dest_path_option = Some(get_unknown_destination_path(destination, file_path));
            debug!("Non-media file will be moved to: {}", dest_path_option.as_ref().unwrap().display());
        } else {
            debug!("Skipping non-media file (copy mode enabled): {}", file_path.display());
        }
    }

    if let Some(final_dest_path) = dest_path_option {
        if let Some(parent) = final_dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        if copy_files {
            info!("Copying file {} to {}", file_path.display(), final_dest_path.display());
            fs::copy(file_path, &final_dest_path)?;
        } else {
            info!("Moving file {} to {}", file_path.display(), final_dest_path.display());
            fs::rename(file_path, &final_dest_path)?;
        }
    } else {
        info!("Skipping file {} (no destination path determined, likely a non-media file in copy mode)", file_path.display());
    }

    Ok(())
}

fn delete_empty_folders(source: &str) -> Result<()> {
    let source_path = Path::new(source);
    for entry in WalkDir::new(source_path).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_dir() {
            let folder_path = entry.path();
            if folder_path.is_dir() && folder_path.read_dir()?.next().is_none() {
                fs::remove_dir(folder_path)?;
            }
        }
    }
    Ok(())
}
fn extract_date(file_path: &Path, use_modified: bool) -> Result<DateTime<Utc>> {
    match extract_exif_date(file_path) {
        Ok(datetime) => {
            debug!("Successfully extracted EXIF date for {}: {:?}", file_path.display(), datetime);
            return Ok(datetime);
        }
        Err(e) => {
            debug!("Failed to extract EXIF date for {}: {}. Falling back to file metadata.", file_path.display(), e);
        }
    }

    debug!("Attempting to use file metadata for {}", file_path.display());
    let metadata = fs::metadata(file_path)
        .with_context(|| format!("Failed to read metadata for {}", file_path.display()))?;

    if use_modified {
        debug!("Using modified time for {}", file_path.display());
        let modified_time = metadata.modified()
            .with_context(|| format!("Failed to get modified time for {}", file_path.display()))?;
        let datetime: DateTime<Utc> = modified_time.into();
        Ok(datetime)
    } else {
        debug!("Using created time for {}", file_path.display());
        let created_time = metadata.created()
            .with_context(|| format!("Failed to get creation time for {}", file_path.display()))?;
        let datetime: DateTime<Utc> = created_time.into();
        Ok(datetime)
    }
}

fn extract_exif_date(file_path: &Path) -> Result<DateTime<Utc>> {
    let file = File::open(file_path).context(format!("EXIF: Failed to open file {}", file_path.display()))?;
    let mut bufreader = BufReader::new(&file);
    let exifreader = exif::Reader::new();
    let exif = exifreader.read_from_container(&mut bufreader).context(format!("EXIF: Failed to read container from {}", file_path.display()))?;

    for &tag in &[
        exif::Tag::DateTimeOriginal,
        exif::Tag::DateTime,
        exif::Tag::DateTimeDigitized,
    ] {
        if let Some(field) = exif.get_field(tag, exif::In::PRIMARY) {
            if let exif::Value::Ascii(ref vec) = field.value {
                if !vec.is_empty() {
                    if let Ok(s) = std::str::from_utf8(&vec[0]) {
                        if s.len() >= 19 {
                            let year: i32 = s[0..4].parse()?;
                            let month: u32 = s[5..7].parse()?;
                            let day: u32 = s[8..10].parse()?;
                            let hour: u32 = s[11..13].parse()?;
                            let minute: u32 = s[14..16].parse()?;
                            let second: u32 = s[17..19].parse()?;
                            return Utc.with_ymd_and_hms(year, month, day, hour, minute, second)
                                .single()
                                .ok_or_else(|| anyhow::anyhow!(
                                    "EXIF: Failed to create unambiguous DateTime for {} (date/time: {}-{}-{} {}:{}:{} might be invalid or ambiguous)", 
                                    file_path.display(), year, month, day, hour, minute, second
                                ));
                        }
                    }
                }
            }
        }
    }
    anyhow::bail!("EXIF: No date found in EXIF data for {}", file_path.display())
}

fn extract_camera_model(file_path: &Path) -> Result<String> {
    let file = File::open(file_path)?;
    let mut bufreader = BufReader::new(&file);
    let exifreader = exif::Reader::new();
    let exif = exifreader.read_from_container(&mut bufreader)?;
    if let Some(field) = exif.get_field(exif::Tag::Model, exif::In::PRIMARY) {
        if let exif::Value::Ascii(ref vec) = field.value {
            if !vec.is_empty() {
                if let Ok(s) = std::str::from_utf8(&vec[0]) {
                    // Clean up model name for folder usage
                    let model = s.trim().replace(char::is_whitespace, "_");
                    return Ok(model);
                }
            }
        }
    }
    // Try make if model not available
    if let Some(field) = exif.get_field(exif::Tag::Make, exif::In::PRIMARY) {
        if let exif::Value::Ascii(ref vec) = field.value {
            if !vec.is_empty() {
                if let Ok(s) = std::str::from_utf8(&vec[0]) {
                    let make = s.trim().replace(char::is_whitespace, "_");
                    return Ok(make);
                }
            }
        }
    }
    anyhow::bail!("No camera model found in EXIF data")
}

fn ensure_unique_filepath(path: PathBuf) -> PathBuf {
    if !path.exists() {
        debug!("Path {} is unique", path.display());
        return path;
    }

    let parent_dir = path.parent().unwrap_or_else(|| Path::new(""));
    
    let filename = path.file_stem()
        .unwrap_or_else(|| OsStr::new("")) 
        .to_str()
        .unwrap_or("");

    let extension = path.extension()
        .unwrap_or_else(|| OsStr::new(""))
        .to_str()
        .unwrap_or("");

    let mut counter = 1;
    loop {
        let new_filename = if extension.is_empty() {
            format!("{}_{}", filename, counter)
        } else {
            format!("{}_{}.{}", filename, counter, extension)
        };
        let candidate_path = parent_dir.join(new_filename);
        if !candidate_path.exists() {
            debug!("Saving file to {} as file with same name already exists.", candidate_path.display());
            return candidate_path;
        }
        counter += 1;
    }
}

fn create_destination_path(
    destination: &str,
    date_time: &DateTime<Utc>,
    camera_model: &str,
    file_path: &Path,
    keep_names: bool,
    camera_model_is_prefix: bool,
) -> Result<PathBuf> {
    let year_str = date_time.year().to_string();
    let month_str = format!("{:02}", date_time.month());
    let day_str = format!("{:02}", date_time.day());

    let mut base_path = PathBuf::from(destination);

    if camera_model_is_prefix && !camera_model.is_empty() {
        base_path.push(camera_model);
    }

    base_path.push(year_str);
    base_path.push(month_str);
    base_path.push(day_str);

    if !camera_model_is_prefix && !camera_model.is_empty() {
        base_path.push(camera_model);
    }
    
    let dest_subfolder_path = base_path;

    let initial_dest_path: PathBuf = if keep_names {
        let original_filename_osstr = file_path.file_name().ok_or_else(|| anyhow::anyhow!("Invalid original filename"))?;
        dest_subfolder_path.join(original_filename_osstr)
    } else {
        let timestamp_str = date_time.format("%Y-%m-%dT%H-%M-%S").to_string();
        let file_ext_str = file_path
            .extension()
            .and_then(OsStr::to_str)
            .unwrap_or("");
        
        let filename = if file_ext_str.is_empty() {
            timestamp_str
        } else {
            format!("{}.{}", timestamp_str, file_ext_str)
        };
        dest_subfolder_path.join(&filename)
    };

    Ok(ensure_unique_filepath(initial_dest_path))
}

fn get_unknown_destination_path(destination: &str, file_path: &Path) -> PathBuf {
    let unknown_path = Path::new(destination).join("unknown");
    fs::create_dir_all(&unknown_path).unwrap();
    let unknown_file_path = unknown_path.join(file_path.file_name().unwrap());
    unknown_file_path
}