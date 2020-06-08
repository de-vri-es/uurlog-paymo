use crate::types;
use reqwest::StatusCode;

pub struct ApiClient {
	pub api_root: String,
	pub auth_token: String,
}

impl ApiClient {
	pub async fn get_clients(&self) -> Result<Vec<types::Client>, String> {
		#[derive(serde::Deserialize)]
		struct Response {
			clients: Vec<types::Client>,
		}

		let response : Response = self.get_auth("clients", "").await?;
		Ok(response.clients)
	}

	pub async fn get_projects_filtered(&self, filter: &ProjectsFilter) -> Result<Vec<types::Project>, String> {
		#[derive(serde::Deserialize)]
		struct Response {
			projects: Vec<types::Project>,
		}

		let response : Response = self.get_auth("projects", &filter.build_query()).await?;
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

		let response : Response = self.get_auth("tasks", "").await?;
		Ok(response.tasks)
	}

	async fn get_auth<T: serde::de::DeserializeOwned>(&self, relative_url: &str, query: &str) -> Result<T, String> {
		let client = reqwest::Client::new();
		let response = client.get(&format!("{}/{}?{}", self.api_root, relative_url, query))
			.basic_auth(&self.auth_token, Some(""))
			.send()
			.await
			.map_err(|e| format!("failed to get {}: error sending request: {}", relative_url, e))?;

		if response.status() != StatusCode::OK {
			Err(format!("failed to get {}: served responded with status code {:?}", relative_url, response.status()))
		} else {
			response.json().await.map_err(|e| format!("failed to get {}: error parsing response {}", relative_url, e))
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
		builder.add_maybe("active", self.active);
		builder.finish()
	}
}

struct FilterBuilder {
	filter: String,
}

impl FilterBuilder {
	fn new() -> Self {
		Self { filter: String::new() }
	}

	fn add(&mut self, key: &str, value: impl std::fmt::Display) {
		if self.filter.is_empty() {
			self.filter = format!("where={}={}", key, value);
		} else {
			self.filter += &format!("and {}={}", key, value);
		}
	}

	fn add_maybe(&mut self, key: &str, value: Option<impl std::fmt::Display>) {
		if let Some(value) = value {
			self.add(key, value)
		}
	}

	fn finish(self) -> String {
		self.filter
	}
}
