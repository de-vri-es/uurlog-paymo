use crate::types;
use reqwest::StatusCode;

pub struct ApiClient {
	pub api_root: String,
	pub auth_token: String,
}

impl ApiClient {
	pub async fn my_user(&self) -> Result<types::User, String> {
		#[derive(serde::Deserialize)]
		struct Response {
			users: Vec<types::User>,
		}

		let mut response : Response = self.get("me", "").await?;
		if response.users.len() != 1 {
			Err(format!("expected exactly 1 user, got {}", response.users.len()))
		} else {
			Ok(response.users.remove(0))
		}
	}

	pub async fn get_clients(&self) -> Result<Vec<types::Client>, String> {
		#[derive(serde::Deserialize)]
		struct Response {
			clients: Vec<types::Client>,
		}

		let response : Response = self.get("clients", "").await?;
		Ok(response.clients)
	}

	pub async fn get_time_entries(&self, filter: &TimeEntryFilter) -> Result<Vec<types::TimeEntry>, String> {
		#[derive(serde::Deserialize)]
		struct Response {
			entries: Vec<types::TimeEntry>,
		}

		let response : Response = self.get("entries", &filter.build_query()).await?;
		Ok(response.entries)
	}

	pub async fn add_entry(&self, task_id: u64, date: uurlog::Date, duration: uurlog::Hours, description: &str) -> Result<(), String> {
		#[derive(serde::Serialize)]
		struct NewTimeEntry<'a> {
			task_id: u64,
			date: &'a str,
			duration: u32,
			description: &'a str,
		}

		let new_entry = NewTimeEntry {
			task_id,
			date: &format!("{}", date),
			duration: duration.total_minutes() * 60,
			description,
		};

		self.post_new("entries", &new_entry).await
	}

	pub async fn delete_entry(&self, entry_id: u64) -> Result<(), String> {
		self.delete("entries", entry_id).await
	}

	pub async fn get_projects_filtered(&self, filter: &ProjectsFilter) -> Result<Vec<types::Project>, String> {
		#[derive(serde::Deserialize)]
		struct Response {
			projects: Vec<types::Project>,
		}

		let response : Response = self.get("projects", &filter.build_query()).await?;
		Ok(response.projects)
	}

	pub async fn get_projects(&self) -> Result<Vec<types::Project>, String> {
		self.get_projects_filtered(&ProjectsFilter::default()).await
	}

	pub async fn get_tasks(&self) -> Result<Vec<types::Task>, String> {
		#[derive(serde::Deserialize)]
		struct Response {
			tasks: Vec<types::Task>,
		}

		let response : Response = self.get("tasks", "").await?;
		Ok(response.tasks)
	}

	async fn get<T: serde::de::DeserializeOwned>(&self, relative_url: &str, query: &str) -> Result<T, String> {
		log::debug!("GET {}/{}?{}", self.api_root, relative_url, query);
		let client = reqwest::Client::new();
		let response = client.get(&format!("{}/{}?{}", self.api_root, relative_url, query))
			.basic_auth(&self.auth_token, Some(""))
			.send()
			.await
			.map_err(|e| format!("failed to get {}: error sending request: {}", relative_url, e))?;

		if response.status() != StatusCode::OK {
			let status = response.status();
			let body = response.text().await.unwrap_or_else(|_| String::new());
			Err(format!("failed to get {}: served responded with status code {:?}: {}", relative_url, status, body))
		} else {
			response.json().await.map_err(|e| format!("failed to get {}: error parsing response {}", relative_url, e))
		}
	}

	async fn post_new(&self, relative_url: &str, body: &impl serde::Serialize) -> Result<(), String> {
		log::debug!("POST {}/{}", self.api_root, relative_url);
		let client = reqwest::Client::new();
		let response = client.post(&format!("{}/{}", self.api_root, relative_url))
			.basic_auth(&self.auth_token, Some(""))
			.json(body)
			.send()
			.await
			.map_err(|e| format!("failed to get {}: error sending request: {}", relative_url, e))?;

		if response.status() != StatusCode::CREATED {
			Err(format!("failed to post {}: served responded with status code {:?}", relative_url, response.status()))
		} else {
			Ok(())
		}
	}

	async fn delete(&self, relative_url: &str, id: u64) -> Result<(), String> {
		log::debug!("DELETE {}/{}/{}", self.api_root, relative_url, id);
		let client = reqwest::Client::new();
		let response = client.delete(&format!("{}/{}/{}", self.api_root, relative_url, id))
			.basic_auth(&self.auth_token, Some(""))
			.send()
			.await
			.map_err(|e| format!("failed to delete {}/{}: error sending request: {}", relative_url, id, e))?;

		if response.status() != StatusCode::OK {
			Err(format!("failed to delete {}/{}: served responded with status code {:?}", relative_url, id, response.status()))
		} else {
			Ok(())
		}
	}
}

#[derive(Debug, Default)]
pub struct ProjectsFilter {
	pub active: Option<bool>,
}

impl ProjectsFilter {
	fn build_query(&self) -> String {
		let mut builder = FilterBuilder::new();
		builder.test_equal("active", self.active);
		builder.finish()
	}
}

#[derive(Debug, Default)]
pub struct TimeEntryFilter {
	pub user_id: Option<u64>,
	pub task_id: Option<u64>,
	pub project_id: Option<u64>,
	pub client_id: Option<u64>,
	pub date: Option<uurlog::Date>,
}

impl TimeEntryFilter {
	pub fn new() -> Self {
		Self::default()
	}

	fn build_query(&self) -> String {
		let mut builder = FilterBuilder::new();
		builder.test_equal("user_id", self.user_id);
		builder.test_equal("task_id", self.task_id);
		builder.test_equal("project_id", self.project_id);
		builder.test_equal("client_id", self.client_id);
		builder.test_in("time_interval", self.date.map(to_time_interval));
		builder.finish()
	}

	pub fn user_id(mut self, val: u64) -> Self {
		self.user_id = Some(val);
		self
	}

	pub fn task_id(mut self, val: u64) -> Self {
		self.task_id = Some(val);
		self
	}

	pub fn project_id(mut self, val: u64) -> Self {
		self.project_id = Some(val);
		self
	}

	pub fn client_id(mut self, val: u64) -> Self {
		self.client_id = Some(val);
		self
	}

	pub fn date(mut self, val: uurlog::Date) -> Self {
		self.date = Some(val);
		self
	}
}

struct FilterBuilder {
	filter: String,
}

impl FilterBuilder {
	fn new() -> Self {
		Self { filter: String::new() }
	}

	fn add_filter(&mut self, filter: std::fmt::Arguments) {
		if self.filter.is_empty() {
			self.filter = format!("where={}", filter);
		} else {
			self.filter += &format!(" and {}", filter);
		}
	}

	fn test_equal(&mut self, key: &str, value: Option<impl std::fmt::Display>) {
		if let Some(value) = value {
			let value = value.to_string();
			self.add_filter(format_args!("{}={}", urlencoding::encode(key), urlencoding::encode(&value)));
		}
	}

	fn test_in(&mut self, key: &str, collection: Option<impl std::fmt::Display>) {
		if let Some(collection) = collection {
			let collection = collection.to_string();
			self.add_filter(format_args!("{} in {}", urlencoding::encode(key), urlencoding::encode(&collection)));
		}
	}


	fn finish(self) -> String {
		self.filter
	}
}

fn to_time_interval(date: uurlog::Date) -> String {
	format!("(\"{}T00:00:00Z\", \"{}T00:00:00Z\")", date, date.next())
}
