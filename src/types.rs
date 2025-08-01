use serde::{Deserialize, Serialize};

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct User {
	pub id: u64,
	pub name: String,
	pub email: String,
	#[serde(rename = "type")]
	pub kind: String,
	pub active: bool,
	pub timezone: String,
	pub phone: String,
	pub skype: Option<String>,
	pub position: String,
	pub workday_hours: Option<f64>,
	pub price_per_hour: Option<f64>,
	pub image: Option<String>,
	pub image_thumb_large: Option<String>,
	pub image_thumb_medium: Option<String>,
	pub image_thumb_small: Option<String>,
	pub date_format: String,
	pub time_format: String,
	pub decimal_sep: String,
	pub thousands_sep: String,
	pub week_start: String,
	pub language: String,
	pub theme: Option<String>,
	pub assigned_projects: Vec<u64>,
	pub managed_projects: Vec<u64>,
	pub is_online: bool,
	pub password: Option<String>,
	pub created_on: String,
	pub updated_on: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Client {
	pub id: u64,
	pub name: String,
	pub address: Option<String>,
	pub city: Option<String>,
	pub state: Option<String>,
	pub postal_code: Option<String>,
	pub country: Option<String>,
	pub phone: Option<String>,
	pub fax: Option<String>,
	pub email: Option<String>,
	pub website: Option<String>,
	pub image: Option<String>,
	pub fiscal_information: Option<String>,
	pub active: bool,
	pub created_on: String,
	pub updated_on: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Project {
	pub id: u64,
	pub name: String,
	pub code: String,
	pub task_code_increment: Option<u64>,
	pub description: String,
	pub client_id: u64,
	pub status_id: Option<u64>,
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

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Task {
	pub id: u64,
	pub name: String,
	pub code: String,
	pub project_id: u64,
	pub tasklist_id: u64,
	pub user_id: u64,
	pub complete: bool,
	pub billable: bool,
	pub seq: u64,
	pub description: String,
	pub price_per_hour: Option<f64>,
	pub due_date: Option<String>,
	pub budget_hours: Option<f64>,
	pub users: Vec<u64>,
	pub created_on: String,
	pub updated_on: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct TimeEntry {
	pub id: u64,
	pub task_id: u64,
	pub user_id: u64,
	pub start_time: Option<String>,
	pub end_time: Option<String>,
	pub description: String,
	pub added_manually: bool,
	pub invoice_item_id: Option<u64>,
	pub billed: bool,
	pub is_bulk: bool,
	pub project_id: u64,
	pub duration: u32,
	pub date: Option<String>,
	pub created_on: String,
	pub updated_on: String,
}
