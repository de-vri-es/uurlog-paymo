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

		let response : Response = self.get_auth("clients").await?;
		Ok(response.clients)
	}

	pub async fn get_projects(&self) -> Result<Vec<types::Project>, String> {
		#[derive(serde::Deserialize)]
		struct Response {
			projects: Vec<types::Project>,
		}

		let response : Response = self.get_auth("projects").await?;
		Ok(response.projects)
	}

	async fn get_auth<T: serde::de::DeserializeOwned>(&self, relative_url: &str) -> Result<T, String> {
		let client = reqwest::Client::new();
		let response = client.get(&format!("{}/{}", self.api_root, relative_url))
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
