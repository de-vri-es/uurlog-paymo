use uurlog::gregorian;
use gregorian::{Date, Year, Month, YearMonth, InvalidDate};
use std::ops::Range;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum PartialDate {
	Year(Year),
	YearMonth(YearMonth),
	YearMonthDay(Date),
}

impl PartialDate {
	pub fn as_range(self) -> Range<Date> {
		match self {
			Self::Year(x) => Range {
				start: x.first_day(),
				end: x.next().first_day(),
			},
			Self::YearMonth(x) => Range {
				start: x.first_day(),
				end: x.next().first_day(),
			},
			Self::YearMonthDay(x) => Range {
				start: x,
				end: x.next(),
			},
		}
	}
}

impl std::str::FromStr for PartialDate {
	type Err = ParsePartialDateError;

	fn from_str(data: &str) -> Result<Self, Self::Err> {
		let mut fields = data.splitn(3, '-');
		let year = fields.next().unwrap();
		let month = fields.next();
		let day = fields.next();

		let year: i16 = year.parse().map_err(|_| InvalidPartialDateSyntax::new())?;

		if let Some(month) = month {
			let month: u8 = month.parse().map_err(|_| InvalidPartialDateSyntax::new())?;
			let month = Month::new(month)?;
			if let Some(day) = day {
				let day: u8 = day.parse().map_err(|_| InvalidPartialDateSyntax::new())?;
				Ok(Self::YearMonthDay(Date::new(year, month, day)?))
			} else {
				Ok(Self::YearMonth(YearMonth::new(year, month)))
			}
		} else {
			Ok(Self::Year(year.into()))
		}
	}
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum ParsePartialDateError {
	InvalidSyntax(InvalidPartialDateSyntax),
	InvalidDate(InvalidDate),
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct InvalidPartialDateSyntax {
	_private: (),
}

impl std::error::Error for ParsePartialDateError {}
impl std::error::Error for InvalidPartialDateSyntax {}

impl InvalidPartialDateSyntax {
	fn new() -> Self {
		Self { _private: () }
	}
}

impl From<InvalidPartialDateSyntax> for ParsePartialDateError {
	fn from(other: InvalidPartialDateSyntax) -> Self {
		Self::InvalidSyntax(other)
	}
}

impl From<InvalidDate> for ParsePartialDateError {
	fn from(other: InvalidDate) -> Self {
		Self::InvalidDate(other)
	}
}

impl From<gregorian::InvalidMonthNumber> for ParsePartialDateError {
	fn from(other: gregorian::InvalidMonthNumber) -> Self {
		Self::InvalidDate(other.into())
	}
}

impl std::fmt::Display for ParsePartialDateError {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		match self {
			Self::InvalidSyntax(e) => write!(f, "{}", e),
			Self::InvalidDate(e) => write!(f, "{}", e),
		}
	}
}

impl std::fmt::Display for InvalidPartialDateSyntax {
	fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
		write!(f, "invalid syntax")
	}
}
