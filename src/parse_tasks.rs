use std::collections::BTreeMap;
use std::path::Path;

pub fn read_task_ids(path: impl AsRef<Path>) -> Result<BTreeMap<String, u64>, String> {
	let path = path.as_ref();
	let data = std::fs::read_to_string(path)
		.map_err(|e| format!("{}", e))?;
	parse_task_ids(&data)
}

pub fn parse_task_ids(data: &str) -> Result<BTreeMap<String, u64>, String> {
	use std::collections::btree_map::Entry;

	let mut result = BTreeMap::new();

	for (i, line) in data.lines().enumerate() {
		let line = line.trim();
		if line.is_empty() || line.starts_with('#') {
			continue;
		}

		let (tag, id) = partition(line, '=')
			.ok_or_else(|| format!("invalid syntax on line {}: expected \"tag = ID\"", i))?;

		let tag = tag.trim();
		let id  = id.trim();

		let id : u64 = id.parse()
			.map_err(|_| format!("invalid task ID on line {}: expected unsigned number, got {}", i, id))?;

		match result.entry(tag.to_string()) {
			Entry::Vacant(x) => {
				x.insert(id);
			},
			Entry::Occupied(_) => {
				return Err(format!("duplicate tag on line {}: {}", i, tag));
			},
		}
	}

	Ok(result)
}

fn partition(input: &str, split: char) -> Option<(&str, &str)> {
	let mut parts = input.splitn(2, split);
	let first = parts.next().unwrap();
	let second = parts.next();

	match second {
		None => None,
		Some(second) => Some((first, second)),
	}
}
