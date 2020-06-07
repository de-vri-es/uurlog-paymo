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
	list_clients: bool,

	#[structopt(long)]
	#[structopt(group = "action")]
	list_projects: bool,

	#[structopt(short, long)]
	token: PathBuf,

	#[structopt(long)]
	#[structopt(requires = "list-projects")]
	client: Option<String>,

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
	} else if options.list_clients {
		list_clients(&api).await
	} else if options.list_projects {
		list_projects(&api).await
	} else {
		unreachable!("no action selected");
	}
}

async fn list_clients(api: &ApiClient) -> Result<(), ()> {
	let mut clients = api.get_clients().await.map_err(|e| eprintln!("{}", e))?;
	clients.sort_by(|a, b| a.name.cmp(&b.name));

	for client in &clients {
		println!("{} ({})", client.name, client.id);
	}
	Ok(())
}

async fn list_projects(api: &ApiClient) -> Result<(), ()> {
	use std::collections::btree_map::Entry::{Occupied, Vacant};

	let mut clients = api.get_clients().await.map_err(|e| eprintln!("{}", e))?;
	clients.sort_by(|a, b| a.name.cmp(&b.name));

	let mut projects_by_client_id = BTreeMap::new();
	let filter = api_client::ProjectsFilter {
		active: Some(true),
	};
	let projects = api.get_projects(&filter).await.map_err(|e| eprintln!("{}", e))?;
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

	for client in &clients {
		println!("{} ({})", client.name, client.id);
		let projects = projects_by_client_id.get(&client.id).map(Vec::as_slice).unwrap_or(&[]);
		for project in projects {
			println!("  {} ({})", project.name, project.id)
		}
	}

	Ok(())
}

async fn upload(api: &ApiClient, file: &Path) -> Result<(), ()> {
	let entries = uurlog::parse_file(file)
		.map_err(|e| eprintln!("Failed to read {}: {}", file.display(), e))?;

	todo!();
}
