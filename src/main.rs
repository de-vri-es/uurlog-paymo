use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use structopt::StructOpt;
use structopt::clap;

mod api_client;
mod types;

use api_client::ApiClient;

#[derive(StructOpt)]
#[structopt(setting = clap::AppSettings::DeriveDisplayOrder)]
#[structopt(setting = clap::AppSettings::UnifiedHelpMessage)]
#[structopt(setting = clap::AppSettings::ColoredHelp)]
#[structopt(group = clap::ArgGroup::with_name("action").required(true))]
struct Options {
	#[structopt(short, long)]
	#[structopt(group = "action")]
	#[structopt(value_name = "FILE")]
	upload: Option<PathBuf>,

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
		upload(&api, &file).await
	} else if options.list_tasks {
		list_tasks(&api).await
	} else {
		unreachable!("no action selected");
	}
}

async fn list_tasks(api: &ApiClient) -> Result<(), ()> {
	use std::collections::btree_map::Entry::{Occupied, Vacant};

	let mut clients = api.get_clients().await.map_err(|e| eprintln!("{}", e))?;
	clients.sort_by(|a, b| a.name.cmp(&b.name));

	let mut projects_by_client_id = BTreeMap::new();
	let filter = api_client::ProjectsFilter {
		active: Some(true),
	};
	let projects = api.get_projects_filtered(&filter).await.map_err(|e| eprintln!("{}", e))?;
	for project in projects {
		match projects_by_client_id.entry(project.client_id) {
			Vacant(entry) => {
				entry.insert(vec![project]);
			},
			Occupied(mut entry) => {
				entry.get_mut().push(project);
			},
		}
	}

	let mut tasks_by_project_id = BTreeMap::new();
	let tasks = api.get_tasks().await.map_err(|e| eprintln!("{}", e))?;
	for task in tasks {
		match tasks_by_project_id.entry(task.project_id) {
			Vacant(entry) => {
				entry.insert(vec![task]);
			},
			Occupied(mut entry) => {
				entry.get_mut().push(task);
			},
		}
	}

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

async fn upload(api: &ApiClient, file: &Path) -> Result<(), ()> {
	let entries = uurlog::parse_file(file)
		.map_err(|e| eprintln!("Failed to read {}: {}", file.display(), e))?;

	let clients = api.get_clients().await.map_err(|e| eprintln!("{}", e))?;
	let projects = api.get_projects().await.map_err(|e| eprintln!("{}", e))?;
	let tasks = api.get_tasks().await.map_err(|e| eprintln!("{}", e))?;

	for entry in &entries {
		let project = if entry.tags.len() == 1 {
			find_unique_matching_task(&entry.tags[0], &tasks, &projects, &clients)?
		} else if entry.tags.len() == 0 {
			eprintln!("entry has no tags, unable to determine project/task");
			eprintln!("  {}", entry);
			return Err(());
		} else {
			eprintln!("entry has multiple tags, unable to determine project/task");
			eprintln!("  {}", entry);
			return Err(());
		};


		eprintln!("{} -> {} ({})", entry, project.name, project.id);
	}

	todo!();
}

fn find_unique_matching_task<'a>(tag: &str, tasks: &'a [types::Task], projects: &[types::Project], clients: &[types::Client]) -> Result<&'a types::Task, ()> {
	let fields : Vec<_> = tag.split("/").collect();
	let (client, project, name) = match fields.len() {
		2 => (None, fields[0], fields[1]),
		3 => (Some(fields[0]), fields[1], fields[2]),
		_ => {
			eprintln!("failed to parse tag: expected [Client/]Project/Task, got {:?}", tag);
			return Err(());
		}
	};

	let client = match client {
		None => None,
		Some(name) => Some(find_unique_matching_client(name, clients)?.id),
	};

	let project = find_unique_matching_project(project, client, projects)?.id;

	let matching_tasks : Vec<_> = tasks
		.iter()
		.filter(|task| task.project_id == project && task.name.eq_ignore_ascii_case(name))
		.collect();

	if matching_tasks.len() == 1 {
		Ok(matching_tasks[0])
	} else if matching_tasks.is_empty() {
		eprintln!("no matching task found for {:?}", tag);
		Err(())
	} else {
		eprintln!("found multiple matching tasks for {:?}", tag);
		Err(())
	}
}

fn find_unique_matching_project<'a>(name: &str, client_id: Option<u64>, projects: &'a [types::Project]) -> Result<&'a types::Project, ()> {
	let matching_projects : Vec<_> = projects
		.iter()
		.filter(|project| client_id.map(|id| project.client_id == id).unwrap_or(true) && project.name.eq_ignore_ascii_case(name))
		.collect();

	if matching_projects.len() == 1 {
		Ok(matching_projects[0])
	} else if matching_projects.is_empty() {
		eprintln!("no matching projects found for {:?}", name);
		Err(())
	} else {
		eprintln!("found multiple matching projects for {:?}", name);
		Err(())
	}
}

fn find_unique_matching_client<'a>(name: &str, clients: &'a [types::Client]) -> Result<&'a types::Client, ()> {
	let matching_clients : Vec<_> = clients
		.iter()
		.filter(|client| client.name.eq_ignore_ascii_case(name))
		.collect();
	if matching_clients.len() == 1 {
		Ok(matching_clients[0])
	} else if matching_clients.is_empty() {
		eprintln!("no matching clients found for {:?}", name);
		Err(())
	} else {
		eprintln!("found multiple matching clients for {:?}", name);
		Err(())
	}
}
