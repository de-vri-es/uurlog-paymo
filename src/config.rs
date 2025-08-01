use std::collections::BTreeMap;
use std::path::Path;

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct Config {
	pub general: GeneralConfig,
	#[serde(alias = "Task")]
	pub tasks: Vec<TaskConfig>,
}

#[derive(serde::Deserialize)]
pub struct GeneralConfig {
	pub token: String,
	pub summarize_per_day: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct TaskConfig {
	pub name: String,
	pub id: u64,
}

impl Config {
	pub fn from_file(path: impl AsRef<Path>) -> Result<Self, ()> {
		use std::io::Read;

		let path = path.as_ref();
		let mut file = std::fs::File::open(path)
			.map_err(|e| log::error!("Failed to open configuration file for reading: {}: {e}", path.display()))?;
		let mut data = Vec::new();
		file.read_to_end(&mut data)
			.map_err(|e| log::error!("Failed to read from configuration file: {}: {e}", path.display()))?;
		let config = toml::from_slice(&data)
			.map_err(|e| log::error!("Failed to parse configuration file: {}: {e}", path.display()))?;
		Ok(config)
	}
}

impl Config {
	pub fn task_ids(&self) -> Result<BTreeMap<&str, u64>, ()> {
		use std::collections::btree_map::Entry;

		let mut output = BTreeMap::new();
		for task in &self.tasks {
			match output.entry(task.name.as_str()) {
				Entry::Occupied(_) => {
					log::error!("Duplicate task name: {}", task.name);
					return Err(());
				},
				Entry::Vacant(entry) => {
					entry.insert(task.id);
				},
			}
		}
		Ok(output)
	}
}
