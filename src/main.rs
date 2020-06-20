use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use structopt::StructOpt;
use structopt::clap;

mod api_client;
mod parse_tasks;
mod partial_date;
mod types;

use api_client::ApiClient;
use partial_date::PartialDate;

#[derive(StructOpt)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
#[structopt(setting = clap::AppSettings::UnifiedHelpMessage)]
#[structopt(setting = clap::AppSettings::ColoredHelp)]
#[structopt(group = clap::ArgGroup::with_name("action").required(true))]
struct Options {
	#[structopt(long, short)]
	#[structopt(parse(from_occurrences))]
	verbose: i8,

	/// Synchronize logged hours to Paymo.
	#[structopt(long)]
	#[structopt(value_name = "FILE")]
	#[structopt(requires = "task-ids")]
	#[structopt(requires = "period")]
	#[structopt(group = "action")]
	sync: Option<PathBuf>,

	/// The period to synchronize.
	#[structopt(value_name = "YYYY[-MM[-DD]]")]
	#[structopt(long)]
	period: Option<PartialDate>,

	/// Print what would be done, without changing any entries on Paymo.
	#[structopt(long)]
	dry_run: bool,

	/// Read tag to task ID mapping from this file.
	#[structopt(long)]
	#[structopt(value_name = "FILE")]
	task_ids: Option<PathBuf>,

	/// List all non-completed tasks for active projects.
	#[structopt(long)]
	#[structopt(group = "action")]
	list_tasks: bool,

	/// Read the Paymo API token from this file.
	#[structopt(short, long)]
	token: PathBuf,

	/// Use this URL as the root for the Paymo API.
	#[structopt(long)]
	#[structopt(default_value = "https://app.paymoapp.com/api")]
	api_root: String,
}

#[tokio::main]
async fn main() {
	if do_main(Options::from_args()).await.is_err() {
		std::process::exit(1);
	}
}

/// Read a file to a string, with a potential final newline removed.
fn read_file(path: impl AsRef<Path>) -> std::io::Result<String> {
	let mut data = std::fs::read_to_string(path)?;
	if data.ends_with('\n') {
		data.pop();
	}
	Ok(data)
}

fn init_logging(verbosity: i8) {
	let level = if verbosity <= -2 {
		log::LevelFilter::Error
	} else if verbosity == -1 {
		log::LevelFilter::Warn
	} else if verbosity == 0 {
		log::LevelFilter::Info
	} else if verbosity == 1 {
		log::LevelFilter::Debug
	} else {
		log::LevelFilter::Trace
	};

	env_logger::from_env("RUST_LOG").filter_module("uurlog_paymo", level).init();
}

async fn do_main(options: Options) -> Result<(), ()> {
	init_logging(options.verbose);

	let token = read_file(&options.token)
		.map_err(|e| log::error!("failed to read token from {}: {}", options.token.display(), e))?;

	let api = ApiClient {
		api_root: options.api_root,
		auth_token: token,
	};

	if let Some(file) = &options.sync {
		sync_to_paymo(
			&api,
			&file,
			&options.task_ids.unwrap(),
			&options.period.unwrap(),
			options.dry_run
		).await
	} else if options.list_tasks {
		list_tasks(&api).await
	} else {
		unreachable!("no action selected");
	}
}

async fn list_tasks(api: &ApiClient) -> Result<(), ()> {
	let mut clients = api.get_clients().await.map_err(|e| log::error!("{}", e))?;
	clients.sort_by(|a, b| a.name.cmp(&b.name));

	// Get all active projects, and index them by client ID.
	let filter = api_client::ProjectsFilter {
		active: Some(true),
	};
	let projects = api.get_projects_filtered(&filter).await.map_err(|e| log::error!("{}", e))?;
	let projects_by_client_id = index_by(projects, |x| x.client_id);

	// Get all tasks, and index them by project ID.
	let tasks = api.get_tasks().await.map_err(|e| log::error!("{}", e))?;
	let tasks_by_project_id = index_by(tasks, |x| x.project_id);

	// Print a tree of clients -> projects -> tasks.
	for client in &clients {
		let projects = projects_by_client_id.get(&client.id);
		if let Some(projects) = projects {
			println!("{} ({})", client.name, client.id);
			for project in projects {
				println!("  {} ({})", project.name, project.id);
				let tasks = tasks_by_project_id.get(&project.id).map(|x| x.as_slice()).unwrap_or_else(|| &[]);
				for task in tasks {
					if !task.complete {
						println!("    {} ({})", task.name, task.id);
					}
				}
			}
		}
	}

	Ok(())
}

/// Synchronize logged hours to Paymo.
async fn sync_to_paymo(api: &ApiClient, file: &Path, task_ids: &Path, period: &PartialDate, dry_run: bool) -> Result<(), ()> {
	let period = period.as_range();

	// Read all entries from the hour log.
	let mut entries = uurlog::parse_file(file)
		.map_err(|e| log::error!("failed to read {}: {}", file.display(), e))?;

	// Filter entries on period.
	entries.retain(|entry| period.contains(&entry.date));

	// Read the tag to task ID mapping from file.
	let task_ids = parse_tasks::read_task_ids(task_ids)
		.map_err(|e| log::error!("failed to read task IDs from {}: {}", task_ids.display(), e))?;

	// Get our Paymo user ID.
	let user = api.my_user().await
		.map_err(|e| log::error!("failed to determine user ID: {}", e))?;

	// Find the right task ID with each hour log entry and index them by date.
	let mut entries_with_tasks = get_tasks_with_entries(&entries, &task_ids)?;

	// Get the existing entries for the period.
	let old_entries = api.get_time_entries(&api_client::TimeEntryFilter::new().user_id(user.id).period(period.clone()))
		.await
		.map_err(|e| log::error!("failed to get time entries between {} and {}: {}", period.start, period.end, e))?;
	log::debug!("found {} existing entries on server between {} and {}", old_entries.len(), period.start, period.end);

	// Collect old entries to delete and new entries to add.
	let mut delete_entries = Vec::new();

	for old_entry in &old_entries {
		// See if there is a matching entry in our own hour log.
		let matching_index = entries_with_tasks
			.iter()
			.position(|(new_entry, _task_id)| {
				new_entry.description == old_entry.description
				&& new_entry.hours.total_minutes() * 60 == old_entry.duration
			});

		// If there is, don't upload that entry.
		if let Some(matching_index) = matching_index {
			entries_with_tasks.remove(matching_index);
		// If there isn't, delete the old entry.
		} else {
			delete_entries.push(old_entry);
		}
	}

	// Delete all old entries without match in the log.
	for &delete_entry in &delete_entries {
		let date = delete_entry.date.as_deref().or_else(|| delete_entry.start_time.as_deref()).unwrap_or("????");
		let hours = uurlog::Hours::from_minutes(delete_entry.duration / 60);
		log::warn!("Deleting entry {}: {}, {}, {}", delete_entry.id, date, hours, delete_entry.description);
		if !dry_run {
			api.delete_entry(delete_entry.id)
				.await
				.map_err(|e| log::error!("{}", e))?;
			tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
		}
	}

	// Upload all new entries without existing entry on Paymo.
	for &(entry, task_id) in &entries_with_tasks {
		log::info!("Adding entry with task id {}: {}", task_id, entry);
		if !dry_run {
			api.add_entry(task_id, entry.date, entry.hours, &entry.description)
				.await
				.map_err(|e| log::error!("{}", e))?;
			tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
		}
	}

	Ok(())
}

/// Find the right task ID for each entry.
fn get_tasks_with_entries<'a>(entries: &'a [uurlog::Entry], task_ids: &BTreeMap<String, u64>) -> Result<Vec<(&'a uurlog::Entry, u64)>, ()> {
	let mut result = Vec::new();

	for entry in entries {
		let task_id = if entry.tags.len() == 1 {
			task_ids.get(&entry.tags[0]).ok_or_else(|| log::error!("unknown task ID for tag: {}", entry.tags[0]))?
		} else if entry.tags.len() == 0 {
			log::error!("entry has no tags, unable to determine project/task");
			log::error!("  {}", entry);
			return Err(());
		} else {
			log::error!("entry has multiple tags, unable to determine project/task");
			log::error!("  {}", entry);
			return Err(());
		};

		result.push((entry, *task_id));
	}

	Ok(result)
}

/// Create an index for a sequence.
///
/// The sequence is indexed based on the return value of the `key` function.
fn index_by<I, F, T, K>(input: I, mut key: F) -> BTreeMap<K, Vec<T>>
where
	I: IntoIterator<Item = T>,
	F: FnMut(&T) -> K,
	K: std::cmp::Ord,
{
	use std::collections::btree_map::Entry;
	let mut result = BTreeMap::new();
	for item in input {
		match result.entry(key(&item)) {
			Entry::Vacant(entry) => {
				entry.insert(vec![item]);
			},
			Entry::Occupied(mut entry) => {
				entry.get_mut().push(item);
			},
		}
	}

	result
}
