use std::collections::BTreeMap;
use std::path::PathBuf;

mod api_client;
mod config;
mod partial_date;
mod types;

use api_client::ApiClient;
use partial_date::PartialDate;

#[derive(clap::Parser)]
struct Options {
	/// Show more messages.
	#[clap(long, short)]
	#[clap(global = true)]
	#[clap(action = clap::ArgAction::Count)]
	verbose: u8,

	/// Show less messages.
	#[clap(long, short)]
	#[clap(global = true)]
	#[clap(action = clap::ArgAction::Count)]
	quiet: u8,

	/// Use the specified configuration file.
	#[clap(long)]
	#[clap(global = true)]
	#[clap(value_name = "FILE.toml")]
	#[clap(default_value = "uurlog-paymo.toml")]
	config: PathBuf,

	/// Use this URL as the root for the Paymo API.
	#[clap(long)]
	#[clap(global = true)]
	#[clap(default_value = "https://app.paymoapp.com/api")]
	api_root: String,

	#[clap(subcommand)]
	command: Subcommand,
}

#[derive(clap::Subcommand)]
enum Subcommand {
	/// List available tasks (organized by client and project).
	ListTasks,

	/// Synchronize logged hours to Paymo.
	Sync(SyncCommand),
}

#[derive(clap::Args)]
struct SyncCommand {
	/// Print what would be done, without changing any entries on Paymo.
	#[clap(long)]
	dry_run: bool,

	/// The period to synchronize.
	#[clap(long)]
	#[clap(value_name = "YYYY[-MM[-DD]]")]
	period: PartialDate,

	/// Load the uurlog file to sync to Paymo.
	#[clap(value_name = "FILE.uurlog")]
	#[clap(required = true)]
	hours: Vec<PathBuf>,
}

#[tokio::main]
async fn main() {
	if do_main(clap::Parser::parse()).await.is_err() {
		std::process::exit(1);
	}
}

fn init_logging(verbose: u8, quiet: u8) {
	let verbosity = i16::from(verbose) - i16::from(quiet);
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

	env_logger::Builder::from_env("RUST_LOG").filter_module("uurlog_paymo", level).init();
}

async fn do_main(options: Options) -> Result<(), ()> {
	init_logging(options.verbose, options.quiet);

	let config = config::Config::from_file(&options.config)?;

	let mut api = ApiClient {
		api_root: options.api_root,
		auth_token: config.general.token.clone(),
		rate_limit: api_client::RateLimit::new(),
	};

	match &options.command {
		Subcommand::ListTasks => {
			list_tasks(&mut api).await
		},
		Subcommand::Sync(command) => {
			sync_to_paymo(command, &config, &mut api).await
		},
	}
}

async fn list_tasks(api: &mut ApiClient) -> Result<(), ()> {
	let mut clients = api.get_clients().await.map_err(|e| log::error!("{e}"))?;
	clients.sort_by(|a, b| a.name.cmp(&b.name));

	// Get all active projects, and index them by client ID.
	let filter = api_client::ProjectsFilter {
		active: Some(true),
	};
	let projects = api.get_projects_filtered(&filter).await.map_err(|e| log::error!("{e}"))?;
	let projects_by_client_id = index_by(projects, |x| x.client_id);

	// Get all tasks, and index them by project ID.
	let tasks = api.get_tasks().await.map_err(|e| log::error!("{e}"))?;
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
async fn sync_to_paymo(command: &SyncCommand, config: &config::Config, api: &mut ApiClient) -> Result<(), ()> {
	let period = command.period.as_range();

	// Read all entries from the hour logs.
	let mut entries = Vec::new();
	for file in &command.hours {
		let file_entries = uurlog::parse_file(file)
			.map_err(|e| log::error!("failed to read {}: {}", file.display(), e))?;
		entries.extend(file_entries);
	}

	// Filter entries on period.
	entries.retain(|entry| period.contains(&entry.date));

	// Read the tag to task ID mapping from file.
	let task_ids = config.task_ids()?;

	// Get our Paymo user ID.
	let user = api.my_user().await
		.map_err(|e| log::error!("failed to determine user ID: {e}"))?;

	// Find the right task ID with each hour log entry and index them by date.
	let mut entries_with_tasks = get_tasks_with_entries(entries, &task_ids)?;

	if let Some(description) = &config.general.summarize_per_day {
		entries_with_tasks = summarize_per_day(entries_with_tasks, description);
	}

	// Get the existing entries for the period.
	let old_entries = api.get_time_entries(&api_client::TimeEntryFilter::new().user_id(user.id).period(period.clone()))
		.await
		.map_err(|e| log::error!("failed to get time entries between {} and {}: {e}", period.start, period.end))?;
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
		let date = delete_entry.date.as_deref().or(delete_entry.start_time.as_deref()).unwrap_or("????");
		let hours = uurlog::Hours::from_minutes(delete_entry.duration / 60);
		log::warn!("Deleting entry {}: {}, {}, {}", delete_entry.id, date, hours, delete_entry.description);
		if !command.dry_run {
			api.delete_entry(delete_entry.id)
				.await
				.map_err(|e| log::error!("{e}"))?;
		}
	}

	// Upload all new entries without existing entry on Paymo.
	for (entry, task_id) in &entries_with_tasks {
		log::info!("Adding entry with task id {task_id}: {entry}");
		if !command.dry_run {
			api.add_entry(*task_id, entry.date, entry.hours, &entry.description)
				.await
				.map_err(|e| log::error!("{e}"))?;
		}
	}

	Ok(())
}

/// Find the right task ID for each entry.
fn get_tasks_with_entries(entries: Vec<uurlog::Entry>, task_ids: &BTreeMap<&str, u64>) -> Result<Vec<(uurlog::Entry, u64)>, ()> {
	let mut result = Vec::new();

	for entry in entries {
		let mut task_ids = entry.tags.iter()
			.filter_map(|tag| Some((tag, task_ids.get(tag.as_str())?)));
		let (task_tag, task_id) = task_ids.next()
			.ok_or_else(|| {
				log::error!("no tag found to determine the paymo project/task");
				log::error!("  {entry}");
			})?;

		if let Some((other_tag, _id)) = task_ids.next() {
			log::error!("multiple tags found that map to a paymo task: {task_tag} and {other_tag}");
			log::error!("  {entry}");
			return Err(())
		}

		result.push((entry, *task_id));
	}

	Ok(result)
}

fn summarize_per_day(entries: Vec<(uurlog::Entry, u64)>, description: &str) -> Vec<(uurlog::Entry, u64)> {
	use std::collections::btree_map::Entry;

	let mut output = BTreeMap::new();
	for (entry, task) in entries {
		match output.entry((entry.date, task)) {
			Entry::Vacant(slot) => {
				let summary_entry = uurlog::Entry {
					date: entry.date,
					hours: entry.hours,
					tags: Vec::new(),
					description: description.into(),
				};
				slot.insert((summary_entry, task));
			},
			Entry::Occupied(mut slot) => {
				let (summary_entry, _task) = slot.get_mut();
				summary_entry.hours += entry.hours;
			},
		}
	}

	output.into_values().collect()
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
