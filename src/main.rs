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
	upload: Option<PathBuf>,

	#[structopt(long)]
	#[structopt(value_name = "FILE")]
	task_ids: Option<PathBuf>,

	#[structopt(long)]
	#[structopt(group = "action")]
	list_tasks: bool,

	#[structopt(short, long)]
	token: PathBuf,

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

	if let Some(file) = &options.upload {
		upload(&api, &file, &options.task_ids.unwrap()).await
	} else if options.list_tasks {
		list_tasks(&api).await
	} else {
		unreachable!("no action selected");
	}
}

async fn list_tasks(api: &ApiClient) -> Result<(), ()> {
	let mut clients = api.get_clients().await.map_err(|e| eprintln!("{}", e))?;
	clients.sort_by(|a, b| a.name.cmp(&b.name));

	let filter = api_client::ProjectsFilter {
		active: Some(true),
	};
	let projects = api.get_projects_filtered(&filter).await.map_err(|e| eprintln!("{}", e))?;
	let projects_by_client_id = index_by(projects, |x| x.client_id);

	let tasks = api.get_tasks().await.map_err(|e| eprintln!("{}", e))?;
	let tasks_by_project_id = index_by(tasks, |x| x.project_id);

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

async fn upload(api: &ApiClient, file: &Path, task_ids: &Path) -> Result<(), ()> {
	let entries = uurlog::parse_file(file)
		.map_err(|e| eprintln!("Failed to read {}: {}", file.display(), e))?;

	let task_ids = parse_tasks::read_task_ids(task_ids)
		.map_err(|e| eprintln!("Failed to read task IDs from {}: {}", task_ids.display(), e))?;

	let user = api.my_user().await
		.map_err(|e| eprintln!("Failed to determine user ID: {}", e))?;

	let entries_with_tasks = get_tasks_with_entries(&entries, &task_ids)?;
	let entries_by_date = index_by(entries_with_tasks, |(entry, _task_id)| entry.date);

	let mut delete_entries = Vec::new();
	let mut post_entries = Vec::new();

	for (date, mut new_entries) in entries_by_date {
		eprintln!("Processing {}", date);

		let old_entries = api.get_time_entries(&api_client::TimeEntryFilter::new().user_id(user.id).date(date))
			.await
			.map_err(|e| eprintln!("failed to get time entries for {}: {}", date, e))?;

		tokio::time::delay_for(std::time::Duration::from_secs(1)).await;

		for old_entry in &old_entries {
			// Skip old entries with start/end time.
			if old_entry.date.is_none() {
				continue;
			}

			let matching_index = new_entries
				.iter()
				.position(|(new_entry, _task_id)| {
					new_entry.description == old_entry.description
					&& new_entry.hours.total_minutes() * 60 == old_entry.duration
				});
			if let Some(matching_index) = matching_index {
				new_entries.remove(matching_index);
			} else {
				delete_entries.push(old_entry.id);
			}
		}

		post_entries.extend(new_entries);
	}

	for &delete_entry in &delete_entries {
		println!("Deleting entry {}", delete_entry);
		api.delete_entry(delete_entry)
			.await
			.map_err(|e| eprintln!("{}", e))?;
		tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
	}

	for &(entry, task_id) in &post_entries {
		println!("Adding entry with task id {}: {}", task_id, entry);
		api.add_entry(task_id, entry.date, entry.hours, &entry.description)
			.await
			.map_err(|e| eprintln!("{}", e))?;
		tokio::time::delay_for(std::time::Duration::from_secs(1)).await;
	}

	Ok(())
}

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
