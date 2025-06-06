use anyhow::Result;
use clap::Parser;
use serde::Deserialize;
use std::{collections::HashMap, path::Path};
use ureq::Agent;

mod local;
mod storage;

/// A file synchronization tool for bunny.net storage zones that synchronizes
/// a local directory with a remote storage zone.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Your bunny.net API key. Use of the env variable strongly recommended
    #[arg(short, long, env = "BUNNYSYNC_API_KEY")]
    api_key: Option<String>,

    /// Your bunny.net storage zone
    #[arg(short, long, env = "BUNNYSYNC_REGION",
    value_parser = clap::builder::PossibleValuesParser::new(["uk", "de", "us_ny", 
    "ny" , "us_la", "la","sg", "se", "br", "sa", "au", "au_syd", "syd"]),
    default_value = "de")]
    region: String,

    /// The source directory or storage zone. Storage zones have prefix zone://
    source: String,

    /// The destination directory or storage zone. Storage zones have prefix zone://
    destination: String,

    /// Perform a dry run
    #[arg(long = "dryrun")]
    dry_run: bool,

    /// Delete files that are not in the source directory
    #[arg(long)]
    delete: bool,

    /// Exclude files that match a pattern. You can use * as a wildcard
    #[arg(long = "exclude", value_parser, num_args = 1.., value_delimiter = ',')]
    exclude: Vec<String>,
}

#[derive(Deserialize)]
struct Config {
    api_key: Option<String>,
    region: Option<String>,
    exclude: Option<Vec<String>>,
}

fn main() {
    let mut args = Args::parse();
    read_config_file(&mut args).expect("reading config file");
    if let Some(api_key) = args.api_key {
        let agent = storage::agent(&api_key).expect("built agent");
        let base_url = storage::base_url(&args.region).expect("invalid region");

        if !is_zone(&args.source) && is_zone(&args.destination) {
            if !Path::new(&args.source).exists() {
                println!("Source path does not exist");
                return;
            }
            if let Err(e) = sync_to_remote(
                &agent,
                &base_url,
                &args.source,
                &args.destination,
                args.dry_run,
                args.delete,
                args.exclude,
            ) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        } else if is_zone(&args.source) && !is_zone(&args.destination) {
            // If the local directory does not exist, throw an error.
            if !Path::new(&args.destination).exists() {
                println!("Destination path does not exist");
                return;
            }
            if let Err(e) = sync_to_local(
                &agent,
                &base_url,
                &args.destination,
                &args.source,
                args.dry_run,
                args.delete,
                args.exclude,
            ) {
                eprintln!("Error: {}", e);
                std::process::exit(1);
            }
        } else {
            println!("Invalid source and destination");
            std::process::exit(1);
        }
        println!("Sync complete");
    } else {
        println!("Please provide an API key");
        return;
    }
}

/// Check for a .bunnysync file in the current directory and if it exists
/// read it and parse it into the args struct.
fn read_config_file(args: &mut Args) -> Result<()> {
    if let Ok(config_file) = std::fs::read_to_string(".bunnysync") {
        let config: Config = toml::from_str(&config_file)?;
        if config.api_key.is_some() {
            args.api_key = config.api_key;
        }
        if let Some(region) = config.region {
            args.region = region;
        }
        if let Some(exclude_list) = config.exclude {
            // Force exclusion of .bunnysync config as it likely contains
            // secrets.
            let mut new_list = exclude_list.clone();
            new_list.push(".bunnysync".into());
            args.exclude = new_list;
        }
    }
    Ok(())
}

fn sync_to_remote(
    agent: &Agent,
    base_url: &str,
    local: &str,
    remote: &str,
    dry_run: bool,
    delete: bool,
    exclude: Vec<String>,
) -> Result<()> {
    let remote = storage::strip_zone_prefix(remote);
    let zone_name = storage::zone_name(remote);
    let remote_files = get_remote_file_map(agent, base_url, &remote, &exclude)?;
    let local_files = get_local_file_map(local, &zone_name, &exclude)?;

    // Update files that are either changed locally or new.
    for (relative_path, local_file) in &local_files {
        // If the file matches an exclude, skip it.
        let file_name = local_file.path.file_name().unwrap().to_str().unwrap();
        if is_excluded(file_name, &exclude) {
            continue;
        }
        // If the file exists and it's not changed, skip it.
        if let Some(destination_file) = remote_files.get(relative_path) {
            if local_file.last_changed <= destination_file.last_changed.and_utc()
                && local_file.length == destination_file.length
            {
                continue;
            }
        }
        if !dry_run {
            // Read the local file and send it to the destination.
            let file_data = std::fs::read(&local_file.path)?;
            storage::put_object(agent, base_url, &remote, &file_data)?;
            println!("Updated: {}", local_file.path.to_string_lossy());
        } else {
            println!("Would update: {}", local_file.path.to_string_lossy());
        }
    }

    // Delete files that are not present locally.
    if delete {
        for (path, _) in remote_files {
            if !local_files.contains_key(&path) {
                if !dry_run {
                    storage::delete_object(agent, base_url, &path)?;
                    println!("Deleted: {}", path);
                } else {
                    println!("Would delete: {}", path);
                }
            }
        }
    }
    Ok(())
}

fn sync_to_local(
    agent: &Agent,
    base_url: &str,
    local: &str,
    remote: &str,
    dry_run: bool,
    delete: bool,
    exclude: Vec<String>,
) -> anyhow::Result<()> {
    let remote = storage::strip_zone_prefix(remote);
    let zone_name = storage::zone_name(&remote);
    let remote_files = get_remote_file_map(agent, base_url, &remote, &exclude)?;
    let local_files = get_local_file_map(local, &zone_name, &exclude)?;

    // Sync the files.
    for (path, remote_file) in &remote_files {
        // If the file exists locally and it's not changed, skip it.
        if let Some(local_file) = local_files.get(path) {
            if local_file.last_changed <= remote_file.last_changed.and_utc()
                && local_file.length == remote_file.length
            {
                continue;
            }
        }

        // Get a local file path for the remote.
        let local_path = local::get_path(local, &zone_name, path);

        if !dry_run {
            // Download the file and save it locally.
            let remote_path = format!("{}/{}", remote_file.path, remote_file.object_name);
            let file_data = storage::get_object(agent, base_url, &remote_path)?;

            // Create the directory if it doesn't exist.

            if let Some(dir) = local_path.parent() {
                if !dir.exists() {
                    std::fs::create_dir_all(dir)?;
                }
            }

            // Write the file.
            std::fs::write(&local_path, file_data)?;
            println!("Updated: {} -> {}", path, &local_path.to_str().unwrap());
        } else {
            println!(
                "Would update: {} -> {}",
                path,
                &local_path.to_str().unwrap()
            );
        }
    }
    // Delete files that are not present remotely.
    if delete {
        for (path, _) in local_files {
            if !remote_files.contains_key(&path) {
                if !dry_run {
                    std::fs::remove_file(&path)?;
                    println!("Deleted: {}", path);
                } else {
                    println!("Would delete: {}", path);
                }
            }
        }
    }
    Ok(())
}

/// Check if the path is a zone.
fn is_zone(path: &str) -> bool {
    path.starts_with("zone://")
}

/// Get the remote files as a map.
fn get_remote_file_map(
    agent: &Agent,
    base_url: &str,
    remote: &str,
    exclude: &[String],
) -> anyhow::Result<HashMap<String, storage::StorageObject>> {
    let remote_files = storage::get_all_objects(agent, base_url, &remote)?;
    // Create a map for quick lookup of destination files.
    let remote_file_map = remote_files
        .into_iter()
        // Skip directories.
        .filter(|file| !file.is_directory)
        // Skip excluded files.
        .filter(|file| !is_excluded(&file.object_name, exclude))
        .map(|file| (format!("{}{}", file.path.clone(), &file.object_name), file))
        .collect();
    Ok(remote_file_map)
}

/// Get the local files as a map.
fn get_local_file_map(
    local: &str,
    zone_name: &str,
    exclude: &[String],
) -> anyhow::Result<HashMap<String, local::LocalFile>> {
    let local_files = local::get_files(local.as_ref())?;
    // Create a map for quick lookup of local files. We construct a destination
    // path from the relative path of the local file.
    let local_file_map: HashMap<_, _> = local_files
        .into_iter()
        // Skip directories.
        .filter(|file| !file.is_directory)
        // Skip excluded files.
        .filter(|file| {
            let filename = file.path.file_name().unwrap().to_str().unwrap();
            !is_excluded(filename, exclude)
        })
        .map(|file| {
            (
                format!(
                    "/{}/{}",
                    zone_name,
                    file.relative_path.to_string_lossy().to_string()
                ),
                file,
            )
        })
        .collect();
    Ok(local_file_map)
}

/// Check if a file is excluded based on the exclude patterns.
fn is_excluded(file_name: &str, exclude_patterns: &[String]) -> bool {
    exclude_patterns
        .iter()
        .any(|pattern| glob_match::glob_match(file_name, pattern))
}
