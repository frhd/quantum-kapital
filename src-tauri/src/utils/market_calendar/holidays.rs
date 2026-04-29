// US equity market full-day holidays (NYSE observed dates).
//
// Maintenance: this list needs to be extended every year. The current window is
// 2025–2028 inclusive. When a year falls off the front, append the next year's
// dates and update the comment below.
//
// Half-day sessions (e.g., the day after Thanksgiving, Christmas Eve when it
// falls on a weekday) are intentionally NOT included — see the module-level
// `Known follow-ups` in the phase doc.

use chrono::NaiveDate;

const fn d(y: i32, m: u32, day: u32) -> NaiveDate {
    match NaiveDate::from_ymd_opt(y, m, day) {
        Some(date) => date,
        // Unreachable — every entry below is hand-checked against the NYSE calendar.
        // If this fires, a maintainer typo'd a date in the HOLIDAYS table.
        None => {
            panic!("invalid hardcoded holiday date — check the most recent entry added to HOLIDAYS")
        }
    }
}

pub const HOLIDAYS: &[NaiveDate] = &[
    // 2025
    d(2025, 1, 1),   // New Year's Day
    d(2025, 1, 20),  // MLK Day
    d(2025, 2, 17),  // Presidents' Day
    d(2025, 4, 18),  // Good Friday
    d(2025, 5, 26),  // Memorial Day
    d(2025, 6, 19),  // Juneteenth
    d(2025, 7, 4),   // Independence Day
    d(2025, 9, 1),   // Labor Day
    d(2025, 11, 27), // Thanksgiving
    d(2025, 12, 25), // Christmas
    // 2026
    d(2026, 1, 1),   // New Year's Day
    d(2026, 1, 19),  // MLK Day
    d(2026, 2, 16),  // Presidents' Day
    d(2026, 4, 3),   // Good Friday
    d(2026, 5, 25),  // Memorial Day
    d(2026, 6, 19),  // Juneteenth
    d(2026, 7, 3),   // Independence Day observed (July 4 is Saturday)
    d(2026, 9, 7),   // Labor Day
    d(2026, 11, 26), // Thanksgiving
    d(2026, 12, 25), // Christmas
    // 2027
    d(2027, 1, 1),   // New Year's Day
    d(2027, 1, 18),  // MLK Day
    d(2027, 2, 15),  // Presidents' Day
    d(2027, 3, 26),  // Good Friday
    d(2027, 5, 31),  // Memorial Day
    d(2027, 6, 18),  // Juneteenth observed (June 19 is Saturday)
    d(2027, 7, 5),   // Independence Day observed (July 4 is Sunday)
    d(2027, 9, 6),   // Labor Day
    d(2027, 11, 25), // Thanksgiving
    d(2027, 12, 24), // Christmas observed (Dec 25 is Saturday)
    // 2028
    d(2028, 1, 17),  // MLK Day (Jan 1 falls on Saturday — markets open Mon Jan 3)
    d(2028, 2, 21),  // Presidents' Day
    d(2028, 4, 14),  // Good Friday
    d(2028, 5, 29),  // Memorial Day
    d(2028, 6, 19),  // Juneteenth
    d(2028, 7, 4),   // Independence Day
    d(2028, 9, 4),   // Labor Day
    d(2028, 11, 23), // Thanksgiving
    d(2028, 12, 25), // Christmas
];
