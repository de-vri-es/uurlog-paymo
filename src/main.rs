use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use structopt::StructOpt;
use structopt::clap;

mod api_client;
mod types;
mod parse_tasks;

use api_client::ApiClient;

#[derive(StructOpt)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
#[structopt(setting = clap::AppSettings::UnifiedHelpMessage)]
#[structopt(setting = clap::AppSettings::ColoredHelp)]
#[structopt(group = clap::ArgGroup::with_name("action").required(true))]
struct Options {
	#[structopt(short, long)]
	#[structopt(value_name = "FILE")]
	#[structopt(requires = "task-ids")]
	#[structopt(group = "action")]
	/// Synchronize logged hours to Paymo.
	sync: Option<PathBuf>,

	#[structopt(long)]
	#[structopt(value_name = "FILE")]
	/// Read tag to task ID mapping from this file.
	task_ids: Option<PathBuf>,

	#[structopt(long)]
	#[structopt(group = "action")]
	/// List all non-completed tasks for active projects.
	list_tasks: bool,

	#[structopt(short, long)]
	/// Read the Paymo API token from this file.
	token: PathBuf,

	#[structopt(long)]
	#[structopt(default_value = "https://app.paymoapp.com/api")]
	/// Use this URL as the root for the Paymo API.
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

async fn do_main(options: Options) -> Result<(), ()> {
	let token = read_file(&options.token)
		.map_err(|e| eprintln!("failed to read token from {}: {}", options.token.display(), e))?;

	let api = ApiClient {
		api_root: options.api_root,
		auth_token: token,
	};

	if let Some(file) = &options.sync {
		sync_to_paymo(&api, &file, &options.task_ids.unwrap()).await
	} else if options.list_tasks {
		list_tasks(&api).await
	} else {
		unreachable!("no action selected");
	}
}

async fn list_tasks(api: &ApiClient) -> Result<(), ()> {
	let mut clients = api.get_clients().await.map_err(|e| eprintln!("{}", e))?;
	clients.sort_by(|a, b| a.name.cmp(&b.name));

	// Get all active projects, and index them by client ID.
	let filter = api_client::ProjectsFilter {
		active: Some(true),
	};
	let projects = api.get_projects_filtered(&filter).await.map_err(|e| eprintln!("{}", e))?;
	let projects_by_client_id = index_by(projects, |x| x.client_id);

	// Get all tasks, and index them by project ID.
	let tasks = api.get_tasks().await.map_err(|e| eprintln!("{}", e))?;
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
async fn sync_to_paymo(api: &ApiClient, file: &Path, task_ids: &Path) -> Result<(), ()> {
	// Read all entries from the hour log.
	let entries = uurlog::parse_file(file)
		.map_err(|e| eprintln!("Failed to read {}: {}", file.display(), e))?;

	// Read the tag to task ID mapping from file.
	let task_ids = parse_tasks::read_task_ids(task_ids)
		.map_err(|e| eprintln!("Failed to read task IDs from {}: {}", task_ids.display(), e))?;

	// Get our Paymo user ID.
	let user = api.my_user().await
		.map_err(|e| eprintln!("Failed to determine user ID: {}", e))?;

	// Find the right task ID with each hour log entry and index them by date.
	let entries_with_tasks = get_tasks_with_entries(&entries, &task_ids)?;
	let entries_by_date = index_by(entries_with_tasks, |(entry, _task_id)| entry.date);

	// Collect old entries to delete and new entries to add.
	let mut delete_entries = Vec::new();
	let mut upload_entries = Vec::new();

	for (date, mut new_entries) in entries_by_date {
		eprintln!("Processing {}", date);

		// Get the entries for the right date.
		let old_entries = api.get_time_entries(&api_client::TimeEntryFilter::new().user_id(user.id).date(date))
			.await
			.map_err(|e| eprintln!("failed to get time entries for {}: {}", date, e))?;

		// Avoid hitting rate limits.
		tokio::time::delay_for(std::time::Duration::from_secs(1)).await;

		for old_entry in &old_entries {
			// Skip old entries with start/end time.
			if old_entry.date.is_none() {
				continue;
			}

			// See if there is a matching entry in our own hour log.
			let matching_index = new_entries
				.iter()
				.position(|(new_entry, _task_id)| {
					new_entry.description == old_entry.description
					&& new_entry.hours.total_minutes() * 60 == old_entry.duration
				});

			// If there is, don't upload that entry.
			if let Some(matching_index) = matching_index {
				new_entries.remove(matching_index);
			// If there isn't, delete the old entry.
			} else {
				delete_entries.push(old_entry.id);
			}
		}

		// Add all remaining new entries to the upload list.
		upload_entries.extend(new_entries);
	}

	// Delete all old entries without match in the log.
	for &delete_entry in &delete_entries {
		println!("Deleting entry {}", delete_entry);
		api.delete_entry(delete_entry)
			.await
			.map_err(|e| eprintln!("{}", e))?;
		tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
	}

	// Upload all new entries without existing entry on Paymo.
	for &(entry, task_id) in &upload_entries {
		println!("Adding entry with task id {}: {}", task_id, entry);
		api.add_entry(task_id, entry.date, entry.hours, &entry.description)
			.await
			.map_err(|e| eprintln!("{}", e))?;
		tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
	}

	Ok(())
}

/// Find the right task ID for each entry.
fn get_tasks_with_entries<'a>(entries: &'a [uurlog::Entry], task_ids: &BTreeMap<String, u64>) -> Result<Vec<(&'a uurlog::Entry, u64)>, ()> {
	let mut result = Vec::new();

	for entry in entries {
		let task_id = if entry.tags.len() == 1 {
			task_ids.get(&entry.tags[0]).ok_or_else(|| eprintln!("unknown task ID for tag: {}", entry.tags[0]))?
		} else if entry.tags.len() == 0 {
			eprintln!("entry has no tags, unable to determine project/task");
			eprintln!("  {}", entry);
			return Err(());
		} else {
			eprintln!("entry has multiple tags, unable to determine project/task");
			eprintln!("  {}", entry);
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
