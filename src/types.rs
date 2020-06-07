use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Client {
	pub id: u64,
	pub name: String,
	pub address: String,
	pub city: String,
	pub state: String,
	pub postal_code: String,
	pub country: String,
	pub phone: String,
	pub fax: String,
	pub email: String,
	pub website: String,
	pub image: Option<String>,
	pub fiscal_information: String,
	pub active: bool,
	pub created_on: String,
	pub updated_on: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
	pub id: u64,
	pub name: String,
	pub code: String,
	pub task_code_increment: u64,
	pub description: String,
	pub client_id: u64,
	pub status_id: u64,
	pub active: bool,
	pub budget_hours: Option<f64>,
	pub price_per_hour: Option<f64>,
	pub billable: bool,
	pub color: String,
	pub users: Vec<u64>,
	pub managers: Vec<u64>,
	pub created_on: String,
	pub updated_on: String,
}
