use reqwest::StatusCode;
use std::error::Error;
use std::time::Duration;
use tokio::time::Instant;

use crate::types;

pub struct ApiClient {
	pub api_root: String,
	pub auth_token: String,
	pub rate_limit: RateLimit,
}

pub struct RateLimit {
	pub decay_period: Duration,
	pub limit: u32,
	pub remaining: u32,
	pub time: Instant,
}

impl ApiClient {
	pub async fn my_user(&mut self) -> Result<types::User, String> {
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

	pub async fn get_clients(&mut self) -> Result<Vec<types::Client>, String> {
		#[derive(serde::Deserialize)]
		struct Response {
			clients: Vec<types::Client>,
		}

		let response : Response = self.get("clients", "").await?;
		Ok(response.clients)
	}

	pub async fn get_time_entries(&mut self, filter: &TimeEntryFilter) -> Result<Vec<types::TimeEntry>, String> {
		#[derive(serde::Deserialize)]
		struct Response {
			entries: Vec<types::TimeEntry>,
		}

		let response : Response = self.get("entries", &filter.build_query()).await?;
		Ok(response.entries)
	}

	pub async fn add_entry(&mut self, task_id: u64, date: uurlog::Date, duration: uurlog::Hours, description: &str) -> Result<(), String> {
		#[derive(serde::Serialize)]
		struct NewTimeEntry<'a> {
			task_id: u64,
			date: &'a str,
			duration: u32,
			description: &'a str,
		}

		let new_entry = NewTimeEntry {
			task_id,
			date: &date.to_string(),
			duration: duration.total_minutes() * 60,
			description,
		};

		self.post_new("entries", &new_entry).await
	}

	pub async fn delete_entry(&mut self, entry_id: u64) -> Result<(), String> {
		self.delete("entries", entry_id).await
	}

	pub async fn get_projects_filtered(&mut self, filter: &ProjectsFilter) -> Result<Vec<types::Project>, String> {
		#[derive(serde::Deserialize)]
		struct Response {
			projects: Vec<types::Project>,
		}

		let response : Response = self.get("projects", &filter.build_query()).await?;
		Ok(response.projects)
	}

	#[allow(dead_code)]
	pub async fn get_projects(&mut self) -> Result<Vec<types::Project>, String> {
		self.get_projects_filtered(&ProjectsFilter::default()).await
	}

	pub async fn get_tasks(&mut self) -> Result<Vec<types::Task>, String> {
		#[derive(serde::Deserialize)]
		struct Response {
			tasks: Vec<types::Task>,
		}

		let response : Response = self.get("tasks", "").await?;
		Ok(response.tasks)
	}

	async fn get<T: serde::de::DeserializeOwned>(&mut self, relative_url: &str, query: &str) -> Result<T, String> {
		self.rate_limit.wait().await;
		log::debug!("GET {}/{}?{}", self.api_root, relative_url, query);
		let client = reqwest::Client::new();
		let response = client.get(format!("{}/{relative_url}?{query}", self.api_root))
			.basic_auth(&self.auth_token, Some(""))
			.header(reqwest::header::ACCEPT, "application/json")
			.send()
			.await
			.map_err(|e| format!("failed to get {relative_url}: error sending request: {e}"))?;
		self.rate_limit.update_from_response(&response);
		log::trace!("RESPONSE {response:#?}");

		if response.status() != StatusCode::OK {
			let status = response.status();
			let body = response.text()
				.await
				.unwrap_or_else(|_| String::new());
			Err(format!("failed to get {relative_url}: served responded with status code {status:?}: {body}"))
		} else {
			response.json()
				.await
				.map_err(|e| format!("failed to get {relative_url}: error parsing response: {e:#}: {:?}", e.source()))
		}
	}

	async fn post_new(&mut self, relative_url: &str, body: &impl serde::Serialize) -> Result<(), String> {
		self.rate_limit.wait().await;
		log::debug!("POST {}/{}", self.api_root, relative_url);
		let client = reqwest::Client::new();
		let response = client.post(format!("{}/{relative_url}", self.api_root))
			.basic_auth(&self.auth_token, Some(""))
			.json(body)
			.send()
			.await
			.map_err(|e| format!("failed to get {relative_url}: error sending request: {e}"))?;
		self.rate_limit.update_from_response(&response);
		log::trace!("RESPONSE {response:#?}");

		if response.status() != StatusCode::CREATED {
			Err(format!("failed to post {relative_url}: served responded with status code {:?}", response.status()))
		} else {
			Ok(())
		}
	}

	async fn delete(&mut self, relative_url: &str, id: u64) -> Result<(), String> {
		self.rate_limit.wait().await;
		log::debug!("DELETE {}/{}/{}", self.api_root, relative_url, id);
		let client = reqwest::Client::new();
		let response = client.delete(format!("{}/{relative_url}/{id}", self.api_root))
			.basic_auth(&self.auth_token, Some(""))
			.send()
			.await
			.map_err(|e| format!("failed to delete {relative_url}/{id}: error sending request: {e}"))?;
		self.rate_limit.update_from_response(&response);
		log::trace!("RESPONSE {response:#?}");

		if response.status() != StatusCode::OK {
			Err(format!("failed to delete {relative_url}/{id}: served responded with status code {:?}", response.status()))
		} else {
			Ok(())
		}
	}
}

impl RateLimit {
	pub fn new() -> Self {
		Self {
			decay_period: Duration::from_secs(1),
			limit: 10,
			remaining: 10,
			time: Instant::now(),
		}
	}

	pub fn update_from_response(&mut self, response: &reqwest::Response) {
		let time = Instant::now();
		let headers = response.headers();
		if let Some(decay_period) = headers.get("X-Ratelimit-Decay-Period") {
			match std::str::from_utf8(decay_period.as_bytes()) {
				Err(e) => log::warn!("failed to parse X-Ratelimit-Decay-Period: invalid UTF-8 in value: {e}"),
				Ok(value) => match value.parse() {
					Err(e) => log::warn!("failed to parse X-Ratelimit-Decay-Period: not a valid number: {e}"),
					Ok(value) => {
						log::debug!("rate limit decay period: {value}");
						self.decay_period = Duration::from_secs_f32(value);
						self.time = time;
					}
				}
			}
		}
		if let Some(rate_limit) = headers.get("X-Ratelimit-Limit") {
			match std::str::from_utf8(rate_limit.as_bytes()) {
				Err(e) => log::warn!("failed to parse X-Ratelimit-Limit: invalid UTF-8 in value: {e}"),
				Ok(value) => match value.parse() {
					Err(e) => log::warn!("failed to parse X-Ratelimit-Limit: not a valid number: {e}"),
					Ok(value) => {
						log::debug!("rate limit per period: {value}");
						self.limit = value
					},
				}
			}
		}
		if let Some(remaining) = headers.get("X-Ratelimit-Remaining") {
			match std::str::from_utf8(remaining.as_bytes()) {
				Err(e) => log::warn!("failed to parse X-Ratelimit-Remaining: invalid UTF-8 in value: {e}"),
				Ok(value) => match value.parse() {
					Err(e) => log::warn!("failed to parse X-Ratelimit-Remaining: not a valid number: {e}"),
					Ok(value) => {
						log::debug!("rate limit remaining: {value}");
						self.remaining = value
					},
				}
			}
		}
	}

	async fn wait(&mut self) {
		if self.remaining == 0 {
			let deadline = self.time + self.decay_period;
			let remaining = deadline.duration_since(Instant::now());
			if !remaining.is_zero() {
				log::debug!("waiting for {remaining:?} to stay within the rate limit");
				tokio::time::sleep(remaining).await;
			}
			self.remaining = 1;
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
	pub period: Option<std::ops::Range<uurlog::Date>>,
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
		builder.test_in("time_interval", self.period.as_ref().map(to_time_interval));
		builder.finish()
	}

	pub fn user_id(mut self, val: u64) -> Self {
		self.user_id = Some(val);
		self
	}

	#[allow(dead_code)]
	pub fn task_id(mut self, val: u64) -> Self {
		self.task_id = Some(val);
		self
	}

	#[allow(dead_code)]
	pub fn project_id(mut self, val: u64) -> Self {
		self.project_id = Some(val);
		self
	}

	#[allow(dead_code)]
	pub fn client_id(mut self, val: u64) -> Self {
		self.client_id = Some(val);
		self
	}

	pub fn period(mut self, val: std::ops::Range<uurlog::Date>) -> Self {
		self.period = Some(val);
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
			self.filter = format!("where={filter}");
		} else {
			self.filter += &format!(" and {filter}");
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

fn to_time_interval(period: &std::ops::Range<uurlog::Date>) -> String {
	format!("(\"{}T00:00:00Z\", \"{}T00:00:00Z\")", period.start, period.end)
}
